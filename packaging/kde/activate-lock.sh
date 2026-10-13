#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] || { echo 'Run as the normal desktop account.'; exit 1; }
printf '%s\n' 'Linux Hello: KDE screen unlock after Win + L.' 'Close camera preview and look straight ahead. The existing protected reference will be used.' 'Sudo authorization comes first, followed by two lock-screen PAM checks. Password fallback remains available.'
sudo -- /usr/local/libexec/linux-hello-sudo/manage-kde.py enable --service kde --user "$(id -un)"
printf '%s\n' 'Ready. After Win + L, show the unlock screen and face the camera. If scanning does not start, submit the empty password field.'
