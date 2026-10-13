#!/usr/bin/env bash
# Fedora source installer. Build and download as your regular desktop user.
set -euo pipefail
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
exec /usr/bin/python3 "$repo/packaging/installer/install.py" "$@"
