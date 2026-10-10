#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
dbus-run-session -- cargo test --locked -p hello-daemon isolated_bus_identity_concurrency_and_denial -- --include-ignored
dbus-run-session -- cargo test --locked -p hello-fingerprint isolated_fprintd_protocol_uses_caller_identity -- --include-ignored
