#!/usr/bin/env python3
"""Record Linux diagnostics; physical capture requires an explicit --device."""

import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent


def read(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def inventory():
    nodes = []
    for node in sorted(Path("/sys/class/video4linux").glob("video*")):
        device = Path("/dev") / node.name
        try:
            visible = stat.S_ISCHR(device.lstat().st_mode)
        except OSError:
            visible = False
        nodes.append({
            "path": str(device),
            "name": read(node / "name"),
            "kernel_device_number": read(node / "dev"),
            "character_device_visible": visible,
            "read_write_access": visible and os.access(device, os.R_OK | os.W_OK),
        })
    missing = []
    for library in ("fontconfig", "freetype2", "xkbcommon", "wayland-client"):
        if not shutil.which("pkg-config") or subprocess.run(
            ["pkg-config", "--exists", library], check=False,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        ).returncode:
            missing.append(library)
    blockers = []
    if missing:
        blockers.append("Missing GUI development libraries: " + ", ".join(missing))
    if nodes and not any(node["character_device_visible"] for node in nodes):
        blockers.append("Kernel reports cameras, but /dev/video* is hidden here. Run in the host desktop terminal.")
    elif nodes and not any(node["read_write_access"] for node in nodes):
        blockers.append("No camera node has current-user read/write access. Inspect session ACLs.")
    elif not nodes:
        blockers.append("No video nodes reported in sysfs; physical camera qualification is pending.")
    return {
        "os": platform.freedesktop_os_release().get("PRETTY_NAME"),
        "kernel": platform.release(),
        "architecture": platform.machine(),
        "machine_type": read("/sys/class/dmi/id/product_name"),
        "bios_version": read("/sys/class/dmi/id/bios_version"),
        "desktop": os.environ.get("XDG_CURRENT_DESKTOP"),
        "session_type": os.environ.get("XDG_SESSION_TYPE"),
        "video_nodes": nodes,
        "missing_gui_libraries": missing,
        "blockers": blockers,
    }


def run_check(name, command, output):
    log = output / (name + ".log")
    print(f"{name}: {' '.join(command)}", flush=True)
    with log.open("w") as stream:
        try:
            result = subprocess.run(command, cwd=ROOT, stdout=stream,
                                    stderr=subprocess.STDOUT, check=False)
            code = result.returncode
        except OSError as error:
            stream.write(str(error) + "\n")
            code = 127
    print(f"{name}: {'passed' if code == 0 else 'failed'} (exit {code})", flush=True)
    return {"name": name, "command": command, "exit_code": code,
            "status": "passed" if code == 0 else "failed", "log": log.name}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path,
                        help="new report directory; existing directories are never overwritten")
    parser.add_argument("--verify", action="store_true", help="run workspace and isolated-bus checks")
    parser.add_argument("--device", help="explicit /dev/videoN for bounded capture, release and reopen")
    parser.add_argument("--offline", action="store_true", help="use only cached Cargo dependencies")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("run on Linux")
    if os.geteuid() == 0:
        parser.error("run as the logged-in desktop user")
    if args.device and not re.fullmatch(r"/dev/video[0-9]+", args.device):
        parser.error("--device must be an exact /dev/videoN path")
    if args.offline:
        # The integration entry point starts its own Cargo processes.
        os.environ["CARGO_NET_OFFLINE"] = "true"
    now = datetime.datetime.now(datetime.timezone.utc)
    output = args.output or ROOT / "work" / ("linux-qualification-" + now.strftime("%Y%m%dT%H%M%S%fZ"))
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        output.mkdir(mode=0o700)
    except FileExistsError:
        parser.error(f"report directory already exists: {output}")
    report = {
        "schema_version": 1, "recorded_at_utc": now.isoformat(),
        "environment": inventory(), "checks": [],
        "selected_device": args.device, "physical_capture": "not_requested",
        "physical_cancellation": "not_requested",
        "authentication_ready": False, "raw_frames_saved": False,
        "manual_checks_pending": ["interactive preview and shutter", "indicator off after cancellation",
                                  "suspend/resume", "unplug/replug", "IR/illumination", "desktop accessibility"],
    }
    for blocker in report["environment"]["blockers"]:
        print("Preflight: " + blocker, flush=True)
    flags = ["--locked"] + (["--offline"] if args.offline else [])
    interrupted = False
    try:
        for tool in ("rustc", "cargo"):
            report["checks"].append(run_check(tool + "-version", [tool, "--version"], output))
        if args.verify:
            commands = [
                ("format", ["cargo", "fmt", "--all", "--", "--check"]),
                ("build", ["cargo", "build", *flags, "--workspace"]),
            ]
            for name, command in commands:
                report["checks"].append(run_check(name, command, output))
            if report["checks"][-1]["exit_code"] == 0:
                for name, command in [
                    ("clippy", ["cargo", "clippy", *flags, "--workspace", "--all-targets", "--", "-D", "warnings"]),
                    ("tests", ["cargo", "test", *flags, "--workspace"]),
                    ("session-bus", ["bash", "tests/integration/run-session.sh"]),
                    ("security", [str(ROOT / "target/debug/hello"), "security"]),
                    ("cameras", [str(ROOT / "target/debug/hello"), "cameras"]),
                ]:
                    report["checks"].append(run_check(name, command, output))
            else:
                # Preserve useful software validation when native camera/GUI
                # dependencies prevent a full build. Full qualification still fails.
                packages = ["hello-core", "hello-face", "hello-ipc", "hello-fingerprint",
                            "hello-pam", "hello-daemon"]
                selection = [arg for package in packages for arg in ("-p", package)]
                for name, command in [
                    ("library-tests", ["cargo", "test", *flags, *selection]),
                    ("library-clippy", ["cargo", "clippy", *flags, *selection,
                                        "--all-targets", "--", "-D", "warnings"]),
                ]:
                    report["checks"].append(run_check(name, command, output))
                report["checks"].append({"name": "workspace-runtime-checks", "status": "blocked",
                                          "reason": "workspace build failed; see build.log"})
        if args.device:
            try:
                visible = stat.S_ISCHR(Path(args.device).lstat().st_mode)
            except OSError:
                visible = False
            if not visible:
                report["physical_capture"] = "blocked"
                report["physical_cancellation"] = "blocked"
                report["checks"].append({"name": "physical-camera", "status": "blocked",
                                          "reason": "selected camera character device is not visible"})
            else:
                # Cargo's hardware test reads this variable and never persists pixels.
                os.environ["LINUX_HELLO_CAMERA"] = args.device
                check = run_check("physical-camera", [
                    "cargo", "test", *flags, "-p", "hello-camera", "--test", "hardware",
                    "selected_camera_captures_releases_and_reopens", "--", "--ignored", "--nocapture",
                ], output)
                report["checks"].append(check)
                report["physical_capture"] = check["status"]
                if check["status"] == "passed":
                    cancelled = run_check("physical-cli-cancellation", [
                        "cargo", "test", *flags, "-p", "hello-cli", "--test", "cli",
                        "selected_camera_cli_cancels_and_reopens", "--", "--ignored", "--nocapture",
                    ], output)
                    report["checks"].append(cancelled)
                    report["physical_cancellation"] = cancelled["status"]
                else:
                    report["physical_cancellation"] = "blocked"
    except KeyboardInterrupt:
        interrupted = True
        report["checks"].append({"name": "qualification", "status": "interrupted"})
        if args.device and report["physical_capture"] == "not_requested":
            report["physical_capture"] = "interrupted"
        if args.device and report["physical_cancellation"] == "not_requested":
            report["physical_cancellation"] = "interrupted"
    finally:
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"Report: {output / 'report.json'}", flush=True)
    failed = any(check["status"] != "passed" for check in report["checks"])
    preflight_blocked = bool(report["environment"]["blockers"]) and not args.verify and not args.device
    return 130 if interrupted else (1 if failed or preflight_blocked else 0)


if __name__ == "__main__":
    sys.exit(main())
