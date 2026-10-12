#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] || { echo 'Run as the normal desktop account.'; exit 1; }
printf '%s\n' 'Linux Hello: odblokowanie KDE po Win + L.' 'Zamknij podglad kamery i patrz prosto. Wykorzystam istniejacy chroniony wzorzec.' 'Najpierw sudo, potem dwa testy PAM blokady. Haslo pozostaje dostepne.'
sudo -- /usr/local/libexec/linux-hello-sudo/manage-kde.py enable --service kde --user "$(id -un)"
printf '%s\n' 'Gotowe. Po Win + L pokaz ekran odblokowania i patrz w kamere. Jesli skan nie ruszy, nacisnij Enter z pustym polem hasla.'
