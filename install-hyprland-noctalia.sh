#!/usr/bin/env bash
# Complete Linux Hello installation for an existing Fedora Hyprland/Noctalia desktop.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
exec /usr/bin/python3 "$repo/packaging/installer/install-hyprland-noctalia.py" "$@"
