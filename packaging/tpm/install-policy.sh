#!/bin/bash
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run with sudo'; exit 1; }
[[ $# -eq 0 ]] || { echo 'No arguments accepted'; exit 1; }
base=$(cd -- "$(dirname -- "$0")" && pwd)
stage=$(mktemp -d /run/linuxhello-tpm-policy.XXXXXX)
trap 'rm -rf -- "$stage"' EXIT
checkmodule -M -m -o "$stage/linuxhello_tpm.mod" "$base/linuxhello_tpm.te"
semodule_package -o "$stage/linuxhello_tpm.pp" -m "$stage/linuxhello_tpm.mod"
semodule -i "$stage/linuxhello_tpm.pp"
echo 'Optional TPM device read/write permission installed for xdm_t. No profile or PAM change.'
