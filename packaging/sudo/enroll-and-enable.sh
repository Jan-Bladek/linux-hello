#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] || { echo 'Start this script as your normal desktop user.'; exit 1; }
user=$(id -un)
uid=$(id -u)
base=/usr/local/libexec/linux-hello-sudo
printf '%s\n' 'Linux Hello: konfiguracja sudo dla Twojego konta.' 'Zamknij podglad kamery w GUI. Wpisz haslo sudo, potem haslo zapisanej osoby z GUI.' 'Po imporcie patrz prosto w kamere; sprawdzanie nie wymaga skretow glowy.' 'To eksperymentalne rozpoznawanie bez zwalidowanej ochrony przed zdjeciem/nagraniem.'
sudo -k
sudo -- "$base/hello-sudo" enroll "$user"
sudo -- "$base/manage.py" enable --uid "$uid"
printf '%s\n' 'Wlaczone. Test sudo: patrz w kamere. Przy braku dopasowania podaj zwykle haslo.'
sudo -k
sudo -- /usr/bin/id -u
printf '%s\n' 'Gotowe. Wylaczenie: sudo /usr/local/libexec/linux-hello-sudo/manage.py disable'
