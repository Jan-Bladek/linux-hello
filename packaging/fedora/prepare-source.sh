#!/usr/bin/env bash
# Prepare a locked, vendored source archive outside the source tree.
# This script downloads Rust dependencies only; it does not install packages,
# activate services, change authentication, or download biometric models.
set -euo pipefail
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
destination=${1:?Usage: bash packaging/fedora/prepare-source.sh /absolute/output/directory}
mkdir -p -- "$destination"
destination=$(cd -- "$destination" && pwd -P)
case "$destination/" in
  "$source_root/"*) printf '%s\n' 'Output must be outside the source tree.' >&2; exit 2 ;;
esac
archive="$destination/linux-hello-0.1.0-dev.tar.gz"
if [[ -e "$archive" ]]; then
  printf '%s\n' 'Archive already exists; choose a new output directory.' >&2
  exit 2
fi
stage=$(mktemp -d)
trap 'test -n "$stage" && test -d "$stage" && rm -rf -- "$stage"' EXIT
mkdir -p -- "$stage/linux-hello-0.1.0-dev"
tar -C "$source_root" --exclude='./.git' --exclude='./target' --exclude='./vendor' --exclude='./work' -cf - . |
  tar -C "$stage/linux-hello-0.1.0-dev" -xf -
cd -- "$stage/linux-hello-0.1.0-dev"
mkdir -p .cargo
cargo vendor --locked --versioned-dirs vendor > .cargo/config.toml
tar -C "$stage" -czf "$archive" linux-hello-0.1.0-dev
printf '%s\n' "$archive"
