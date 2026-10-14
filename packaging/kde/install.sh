#!/bin/bash
set -euo pipefail
# Historical partial installers are deliberately disabled: they could replace
# an active PAM module with an incompatible feature set or a mixed bundle.
printf '%s\n' 'This partial installer is retired. As your regular user, run ./install.sh from the repository root.' >&2
exit 1
