"""Recorder boundaries: no implicit capture, honest failures, no overwritten reports."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "qualification", Path(__file__).resolve().parents[1] / "tools/qualify-linux.py"
)
qualification = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qualification)


class QualificationTests(unittest.TestCase):
    def invoke(self, output, *args, failed=None):
        commands = []
        failed_names = {failed} if isinstance(failed, str) else set(failed or [])

        def check(name, command, directory):
            commands.append(command)
            return {"name": name, "status": "failed" if name in failed_names else "passed",
                    "exit_code": 1 if name in failed_names else 0}

        with patch("sys.argv", ["qualify-linux.py", "--output", str(output), *args]), \
             patch.object(qualification.os, "geteuid", return_value=1000), \
             patch.object(qualification, "inventory", return_value={"blockers": []}), \
             patch.object(qualification, "run_check", side_effect=check):
            result = qualification.main()
        return result, commands

    def test_default_and_software_verification_never_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            for index, args in enumerate([[], ["--verify"]]):
                output = Path(directory) / str(index)
                result, commands = self.invoke(output, *args)
                self.assertEqual(result, 0)
                self.assertTrue(all("hardware" not in command and "capture" not in command
                                    and "probe" not in command for command in commands))
                report = json.loads((output / "report.json").read_text())
                self.assertEqual(report["physical_capture"], "not_requested")
                self.assertFalse(report["authentication_ready"])

    def test_failed_build_preserves_library_checks_and_returns_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "report"
            result, commands = self.invoke(output, "--verify", failed="build")
            self.assertEqual(result, 1)
            self.assertFalse(any("security" in command for command in commands))
            library_tests = [command for command in commands if "test" in command]
            self.assertEqual(len(library_tests), 1)
            self.assertIn("hello-face", library_tests[0])
            self.assertNotIn("--workspace", library_tests[0])
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(report["checks"][-1]["status"], "blocked")

    def test_failed_library_tests_are_recorded_as_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "report"
            result, _ = self.invoke(output, "--verify", failed=("build", "library-tests"))
            self.assertEqual(result, 1)
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(next(check for check in report["checks"]
                                  if check["name"] == "library-tests")["status"], "failed")

    def test_existing_report_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            sentinel = output / "report.json"
            sentinel.write_text("original report")
            with self.assertRaises(SystemExit) as error:
                self.invoke(output)
            self.assertEqual(error.exception.code, 2)
            self.assertEqual(sentinel.read_text(), "original report")

    def test_hidden_selected_camera_is_blocked_before_cargo_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "report"
            with patch.object(Path, "lstat", side_effect=FileNotFoundError):
                result, commands = self.invoke(output, "--device", "/dev/video99999")
            self.assertEqual(result, 1)
            self.assertFalse(any("hardware" in command for command in commands))
            report = json.loads((output / "report.json").read_text())
            self.assertEqual(report["physical_capture"], "blocked")


if __name__ == "__main__":
    unittest.main()
