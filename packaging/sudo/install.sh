#!/bin/bash
set -euo pipefail
# Run explicitly with sudo after building the optional binaries. No PAM changes.
[[ $EUID -eq 0 ]] || { echo 'Run with sudo'; exit 1; }
[[ $# -eq 1 && -d $1 ]] || { echo 'Usage: install.sh ABSOLUTE_REPOSITORY'; exit 1; }
repo=$1
[[ $repo = /* ]] || exit 1
base=/usr/local/libexec/linux-hello-sudo
state=/var/lib/linux-hello/sudo
# Reject pre-existing links or writable roots rather than changing their owner.
python3 - "$base" "$state" <<'PY'
import os,sys,stat
from pathlib import Path
for name in sys.argv[1:]:
 p=Path(name)
 for part in [*reversed(p.parents),p]:
  if not part.exists() and not part.is_symlink(): continue
  m=part.lstat()
  if not stat.S_ISDIR(m.st_mode) or m.st_uid != 0 or m.st_mode & 0o022:
   raise SystemExit(f'Unsafe install directory: {part}')
PY
install -d -m 0755 "$base" "$base/models"
install -d -m 0700 /var/lib/linux-hello "$state"
install -m 0755 "$repo/target/release/hello-sudo" "$base/hello-sudo"
install -m 0755 "$repo/target/release/hello-face-worker" "$base/hello-face-worker"
install -m 0644 "$repo/target/release/libhello_pam.so" /usr/lib64/security/pam_linuxhello.so
install -m 0755 "$repo/packaging/sudo/manage.py" "$base/manage.py"
install -m 0755 "$repo/packaging/sudo/enroll-and-enable.sh" "$base/enroll-and-enable.sh"
for name in face_detection_yunet_2023mar.onnx face_recognition_sface_2021dec.onnx; do
 install -m 0644 "$repo/work/inference/models/$name" "$base/models/$name"
done
install -m 0600 /dev/null "$state/.anchor"
restorecon -R "$base" /var/lib/linux-hello /usr/lib64/security/pam_linuxhello.so
printf '%s\n' 'Installed. sudo PAM unchanged. Next: fresh sudo hello-sudo enroll USER, then manage.py enable --uid UID.'
