#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] || { echo 'Run as the normal desktop account.'; exit 1; }
login_user=$(id -un)
printf '%s\n' 'Linux Hello: aktywacja logowania KDE dla Twojego konta.' 'Zamknij podglad kamery w GUI. Usiadz jak podczas zapisywania wzorca i patrz prosto.' 'Wykorzystam istniejacy wzorzec sudo; nowe skanowanie/zapisywanie twarzy nie jest potrzebne.' 'Aktywacja wymaga dwoch poprawnych prob z Twoja twarza. W razie odrzucenia PAM pozostaje bez zmian.' 'To eksperymentalna biometria bez zwalidowanej ochrony przed zdjeciem/nagraniem.'
sudo -- /usr/local/libexec/linux-hello-sudo/manage-kde.py enable --user "$login_user"
printf '%s\n' 'Gotowe. Przy nastepnym logowaniu wybierz konto i nacisnij Enter z pustym polem hasla, patrzac w kamere.' 'Haslo nadal dziala. KWallet moze poprosic osobno o swoje haslo.'
