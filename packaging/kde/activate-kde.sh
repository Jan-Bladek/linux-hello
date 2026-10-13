#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] || { echo 'Run as the normal desktop account.'; exit 1; }
login_user=$(id -un)
printf '%s\n' 'Linux Hello: KDE sign-in activation for your account.' 'Close camera preview. Sit as you did during enrollment and look straight ahead.' 'The existing sudo reference will be used; new face enrollment is not needed.' 'Activation requires two successful face checks. If rejected, PAM remains unchanged.' 'Experimental biometrics without validated photo/video attack protection.'
printf '%s\n' 'Sudo authorization first (face or password), then two separate sign-in checks with a countdown.' 'Do not press Enter during scanning; wait for the result.'
sudo -- /usr/local/libexec/linux-hello-sudo/manage-kde.py enable --user "$login_user"
printf '%s\n' 'Ready. At your next sign-in, select the account and submit the empty password field while facing the camera.' 'Password fallback remains available. KWallet may request its own password.'
