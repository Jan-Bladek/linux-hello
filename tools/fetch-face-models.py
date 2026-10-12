#!/usr/bin/env python3
"""Explicit download of pinned OpenCV Zoo evaluation artifacts; never called at runtime."""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request

COMMIT = "47534e27c9851bb1128ccc0102f1145e27f23f98"
ARTIFACTS = (
    ("face_detection_yunet", "face_detection_yunet_2023mar.onnx", 232589,
     "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"),
    ("face_recognition_sface", "face_recognition_sface_2021dec.onnx", 38696353,
     "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"),
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path,
                        help="new directory for local evaluation (must not already exist)")
    args = parser.parse_args()
    args.output.mkdir(mode=0o700, parents=False, exist_ok=False)
    report = {"repository": "https://github.com/opencv/opencv_zoo", "commit": COMMIT,
              "scope": "local diagnostic evaluation; dataset provenance unresolved; no authentication",
              "artifacts": []}
    for directory, name, size, digest in ARTIFACTS:
        url = f"https://media.githubusercontent.com/media/opencv/opencv_zoo/{COMMIT}/models/{directory}/{name}"
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read(size + 1)
        if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f"artifact verification failed: {name}")
        with (args.output / name).open("xb") as destination:
            destination.write(data)
        base = f"https://raw.githubusercontent.com/opencv/opencv_zoo/{COMMIT}/models/{directory}"
        for document in ("LICENSE", "README.md"):
            with urllib.request.urlopen(f"{base}/{document}", timeout=60) as response:
                data = response.read(128 * 1024 + 1)
            if len(data) > 128 * 1024:
                raise ValueError("upstream documentation exceeds limit")
            (args.output / f"{directory}-{document}").write_bytes(data)
        report["artifacts"].append({"name": name, "bytes": size, "sha256": digest, "url": url})
    (args.output / "provenance.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
