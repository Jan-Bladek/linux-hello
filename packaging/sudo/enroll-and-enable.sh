#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] || { echo 'Start this script as your normal desktop user.'; exit 1; }
user=$(id -un)
uid=$(id -u)
base=/usr/local/libexec/linux-hello-sudo
printf '%s\n' 'Linux Hello: sudo setup for your account.' 'Close camera preview. Enter your account password, followed by the GUI reference passphrase.' 'After import, look straight at the camera; verification does not require head turns.' 'Experimental recognition without validated photo/video attack protection.'
sudo -k
sudo -- "$base/hello-sudo" enroll "$user"
sudo -- "$base/manage.py" enable --uid "$uid"
printf '%s\n' 'Enabled. Sudo test: face the camera. Enter your password if recognition fails.'
sudo -k
sudo -- /usr/bin/id -u
printf '%s\n' 'Ready. Global sudo disable: sudo /usr/local/libexec/linux-hello-sudo/manage.py disable'
