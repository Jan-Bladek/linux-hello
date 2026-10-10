# Test entry points

Tests live next to the crate whose boundary they validate, so Cargo discovers and
runs them without a custom workspace-wide test harness.

| Area | Location | Coverage |
| --- | --- | --- |
| Camera abstraction | `crates/hello-camera/src/tests.rs` | Simulated acquisition, bounded queue, invalid frames, timeout, drop/stop, reconnect identity, cancellation-safe terminal errors |
| V4L2 input validation | `crates/hello-camera/src/linux.rs` | Device path validation and sanitized metadata |
| Physical camera | `crates/hello-camera/tests/hardware.rs` | Explicitly ignored capture/release/reopen test; requires operator-selected device |
| Security state machine | `crates/hello-core/src/lib.rs` | Context, order, challenge binding, expiry, PAD failure, authentication disabled |
| Biometric primitives | `crates/hello-face/src/lib.rs` | Numeric/quality bounds, model/dimension binding, synthetic enrollment and matching |
| Encrypted storage | `crates/hello-storage/src` | Randomized owner-bound AEAD, byte tampering/truncation, atomic save/delete, permissions, symlink and hard-link rejection |
| IPC | `crates/hello-ipc/src/lib.rs` | Bounded strict response decoding, disabled capabilities |
| Real D-Bus transport | `crates/hello-daemon/src/lib.rs` | Bus-derived UID, concurrent requests, missing privileged methods, invalid arguments |
| fprintd protocol | `crates/hello-fingerprint/src/lib.rs` | Test-only service on isolated bus; caller identity selected using empty username |
| CLI integration | `crates/hello-cli/tests/cli.rs` | Real executable output, input bounds, unavailable daemon failure |
| Slint rendering | `apps/hello-settings/src/main.rs` | Actual compiled layouts and software rendering: five tabs at 980×740 and 760×660 |

Run `cargo test --workspace --locked` followed by
`bash tests/integration/run-session.sh`. The latter runs the two tests that need
an isolated D-Bus. Hardware remains ignored and must never run automatically on
a user's camera. No test installs PAM or changes system authentication.

For UI visual review, set `LINUX_HELLO_UI_SNAPSHOT_DIR` to a scratch directory and
run `cargo test --locked -p hello-settings`. The test writes PPM images of the
camera-off UI. These are application screenshots, not camera or biometric frames.
Do not set this variable in ordinary runtime processes.

Synthetic values test software invariants only. Physical biometric accuracy,
presentation attacks, Fedora login/recovery and RPM installation require separate
hardware/VM reports. See `docs/validation.md` for the checks actually executed.
