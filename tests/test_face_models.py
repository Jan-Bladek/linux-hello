"""Download boundaries without network access or real weights."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("fetch_face_models", Path(__file__).resolve().parents[1] / "tools/fetch-face-models.py")
fetch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fetch)


class ModelDownloadTests(unittest.TestCase):
    def run_fetch(self, output, payload):
        artifact = ("fixture", "model.onnx", 4, hashlib.sha256(b"test").hexdigest())
        def respond(url, **kwargs):
            return io.BytesIO(payload if url.endswith(".onnx") else b"upstream notice")
        with mock.patch.object(fetch, "ARTIFACTS", (artifact,)), \
             mock.patch("sys.argv", ["fetch-face-models", "--output", str(output)]), \
             mock.patch.object(fetch.urllib.request, "urlopen", side_effect=respond) as network, \
             contextlib.redirect_stdout(io.StringIO()):
            fetch.main()
        return network

    def test_valid_artifact_records_pins_and_notices(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "models"
            network = self.run_fetch(output, b"test")
            self.assertEqual((output / "model.onnx").read_bytes(), b"test")
            report = json.loads((output / "provenance.json").read_text())
            self.assertEqual(report["commit"], fetch.COMMIT)
            self.assertEqual(report["artifacts"][0]["sha256"], hashlib.sha256(b"test").hexdigest())
            self.assertEqual((output / "fixture-LICENSE").read_bytes(), b"upstream notice")
            self.assertEqual(network.call_count, 3)

    def test_truncated_oversized_and_changed_models_never_get_written(self):
        for payload in (b"tes", b"test!", b"evil"):
            with self.subTest(payload=payload), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "models"
                with self.assertRaisesRegex(ValueError, "artifact verification failed"):
                    self.run_fetch(output, payload)
                self.assertFalse((output / "model.onnx").exists())
                self.assertFalse((output / "provenance.json").exists())

    def test_existing_directory_rejected_before_network(self):
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(fetch.urllib.request, "urlopen") as network, \
             mock.patch("sys.argv", ["fetch-face-models", "--output", directory]):
            with self.assertRaises(FileExistsError):
                fetch.main()
            network.assert_not_called()


if __name__ == "__main__":
    unittest.main()
