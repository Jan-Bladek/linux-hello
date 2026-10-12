#!/bin/bash
set -euo pipefail
[[ $EUID -eq 0 && $# -eq 1 && $1 = /* ]] || { echo 'sudo install.sh ABSOLUTE_REPOSITORY'; exit 1; }
repo=$1
base=/usr/local/libexec/linux-hello-sudo
python3 "$base/manage.py" status
# Stage binaries in the root-owned installation. No PAM enablement here.
install -m 0755 "$repo/target/release/hello-sudo" "$base/hello-sudo"
install -m 0644 "$repo/target/release/libhello_pam.so" /usr/lib64/security/pam_linuxhello.so
install -m 0755 "$repo/packaging/kde/manage-kde.py" "$base/manage-kde.py"
install -m 0755 "$repo/packaging/kde/activate-kde.sh" "$base/activate-kde.sh"
install -m 0755 "$repo/packaging/kde/activate-lock.sh" "$base/activate-lock.sh"
# This small launcher only verifies the kernel real UID; no user commands,
# target identities, enrollment, paths or diagnostic options are accepted.
install -m 4755 "$repo/target/release/hello-lock-verify" "$base/hello-lock-verify"
install -m 0755 "$repo/work/kde/kde-pam-probe" "$base/kde-pam-probe"
install -m 0644 "$repo/work/kde/linuxhello_kde.pp" "$base/linuxhello_kde.pp"
semodule -i "$base/linuxhello_kde.pp"
# Only the root-only diagnostic probe needs a direct xdm_t entrypoint for tests.
# The inference helper/worker retain their original contexts and confinement.
if semanage fcontext -l -C | awk '{print $1}' | /usr/bin/grep -Fx "$base/kde-pam-probe" >/dev/null; then
 semanage fcontext -m -t xdm_exec_t "$base/kde-pam-probe"
else
 semanage fcontext -a -t xdm_exec_t "$base/kde-pam-probe"
fi
restorecon /usr/lib64/security/pam_linuxhello.so "$base/manage-kde.py" "$base/kde-pam-probe" "$base/hello-lock-verify" "$base/activate-lock.sh"
printf '%s\n' 'Installed KDE adapter and MMAP policy. Login PAM unchanged until manage-kde.py enable succeeds.'
