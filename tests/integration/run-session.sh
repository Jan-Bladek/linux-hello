#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
dbus-run-session -- cargo test --locked -p hello-daemon isolated_bus_identity_concurrency_and_denial -- --include-ignored
dbus-run-session -- cargo test --locked -p hello-fingerprint isolated_fprintd_protocol_uses_caller_identity -- --include-ignored
# Slint portal settings must work outside Tokio, including old portals without ReadOne.
dbus-run-session -- cargo test --locked -p hello-settings portal_bus_modern_and_legacy_reads_without_tokio -- --include-ignored
