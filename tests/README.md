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
| Diagnostic inference coordinator | `crates/hello-face/src/pipeline.rs` | Validated stage ordering, model contract, detector geometry, tensor shape/finite values, output binding; test-only engine |
| Encrypted storage | `crates/hello-storage/src` | Randomized owner-bound AEAD, byte tampering/truncation, atomic save/delete, permissions, symlink and hard-link rejection |
| IPC | `crates/hello-ipc/src/lib.rs` | Bounded strict response decoding, disabled capabilities |
| Real D-Bus transport | `crates/hello-daemon/src/lib.rs` | Bus-derived UID, concurrent requests, missing privileged methods, invalid arguments |
| fprintd protocol | `crates/hello-fingerprint/src/lib.rs` | Test-only service on isolated bus; caller identity selected using empty username |
| CLI integration | `crates/hello-cli/tests/cli.rs` | Real executable output, input bounds, unavailable daemon failure |
| Physical CLI cancellation | `crates/hello-cli/tests/cli.rs` | Explicitly ignored SIGINT shutdown/reopen; requires `LINUX_HELLO_CAMERA` |
| Slint rendering | `apps/hello-settings/src/main.rs` | Actual compiled layouts and software rendering: five tabs at 980×740 and 760×660 |

Run `cargo test --workspace --locked` followed by
`bash tests/integration/run-session.sh`. The latter runs the two tests that need
an isolated D-Bus. Hardware remains ignored and must never run automatically on
a user's camera. These ordinary Cargo/bus tests do not install PAM or change
system authentication. Explicit privileged and hardware entry points below have
separate requirements.

For UI visual review, set `LINUX_HELLO_UI_SNAPSHOT_DIR` to a scratch directory and
run `cargo test --locked -p hello-settings`. The test writes PPM images of the
camera-off UI. These are application screenshots, not camera or biometric frames.
Do not set this variable in ordinary runtime processes.

The diagnostic tools use Python 3's standard library. Run their boundary tests
with `python3 -B -m unittest discover -s tests -p 'test_*.py'`. This includes
USB descriptor scope/length checks and tests that UVC metadata requests allow
only GET_LEN/GET_INFO. Tests do not access cameras. Actual read-only metadata
inspection is documented in [IR qualification](../docs/ir-qualification.md).
They verify no implicit capture, continued library validation after failed builds,
honest library-failure reporting, preservation of existing
reports, and rejection of a hidden camera before capture. They use mocked
commands and never access a physical camera. See
[hardware support](../docs/hardware-support.md#lenovo-qualification-procedure)
for recording the actual host checks.

The scoped emitter hardware test performs explicit mode writes and capture, then
checks the original register bytes after stop and after failed setup/Drop. Run it
only for the authorized, tested Chicony IR device:

```sh
LINUX_HELLO_CAMERA=/dev/video2 cargo test --locked -p hello-camera \
  selected_ir_mode_captures_and_restores_on_stop_and_drop -- --ignored
```

It never saves frames. Unit tests cover descriptor GUID/interface/selector scope
and rejection of incompatible capability payloads. Native CLI SIGINT/SIGTERM,
cooperative exclusion and RGB rejection results are recorded in
`work/inference/emitter-native-verification.json` on the qualifying workstation.

Synthetic values test software invariants only. Physical biometric accuracy,
presentation attacks, Fedora login/recovery and RPM installation require separate
hardware/VM reports. See `docs/validation.md` for the checks actually executed.

## Optional native inference

See [inference diagnostics](../docs/inference.md) for Fedora OpenCV prerequisites,
optional-feature tests and the explicitly selected public-fixture integration test.
Default tests never download weights or start a camera. Python download-boundary
tests run with `python3 -B -m unittest discover -s tests -p "test_*.py"`.

The optional CLI tests exercise supervised child timeout/crash/cancellation,
blocked pipes and reply limits, real CPU/memory/syscall enforcement, and held-out
evaluation arithmetic/data leakage. The ignored `crates/hello-cli/tests/worker.rs`
test needs the same explicit model/public-fixture environment as the direct
model integration test. It runs actual inference with the worker restrictions.

## Optional verification and PAM checks

The current [authentication flow](../docs/authentication-flow.md) distinguishes
GUI verification, root sudo/display-manager verification and own-UID locker
verification. Test optional feature builds separately:

```sh
cargo test --locked -p hello-pam --features kde-pam
cargo test --locked -p hello-sudo --features sudo-face
cargo test --locked -p hello-cli --features face-opencv passive
cargo test --locked -p hello-settings --features face-diagnostics
```

PAM tests cover supported caller/service scope, per-transaction credential
state, failure and expired-child rejection. Passive tests cover three-frame
success, unknown identity, quality recovery, duplicates, multiple faces,
reference changes, cancellation and model errors. Fixtures are synthetic and
do not establish NIR accuracy or attack resistance.

Root-only manual dispatch checks are under `packaging/sudo` and `packaging/kde`.
Use their [sudo](../docs/sudo-integration.md#recovery-and-testing) and
[KDE](../docs/kde-login.md#status-rollback-and-checks) instructions. The
`test-setcred.py` fixture mounts substitute helpers/dummy state only inside a
verified private mount namespace. Never replace the live verifier globally.
Missing-marker tests have documented pre-opt-in assumptions; do not run them
over an already active service without reviewing their state requirements.

The explicit password-fallback test temporarily moves the detector and restores
it in `finally`; it needs authorized root access and the original account's
masked password. SIGKILL/power loss still requires recovery. It opens no desktop
session and does not measure face recognition. New deployment stacks also need
disposable-VM recovery tests. The owner confirmed real login and Win + L on the
target Fedora KDE host; broader hardware/desktop coverage remains open.

## Documentation checks

```sh
python3 tools/check-docs.py
python3 -B -m unittest discover -s tests -p 'test_documentation.py'
```

The checker validates repository Markdown file/image links and heading fragments,
including archived milestones. It skips fenced examples and does not fetch
external URLs. CI runs the same check. It cannot validate external source claims,
biometric security or the truth of owner-declared results.

## Unified person setup

`tests/test_person_setup.py` checks exact boolean choices, bounded private secret
framing, rollback of the old profile/PAM after partial failure or result-pipe
disconnection, and own-account sudo disabling without changing other users.
The GUI requires an exact completed-choice response and a successful root process
exit before reporting completion. Helper tests cover truncated/oversized input
and preserving existing higher thresholds. Rendering tests exercise ordinary
user pages; developer controls remain behind an explicit switch.

The privileged namespace regression also checks actual sudo PAM dispatch with
missing, valid and broken per-account disable markers; another user's disable
cannot affect this account. It remains a simulated verifier test, not a live
biometric or complete Polkit GUI acceptance test.

## Real TPM checks

See [TPM protection](../docs/tpm.md) for the explicitly invoked root-only real-TPM
regression and optional KDE key-release context checks. They use synthetic data
and private mount namespaces; ordinary tests do not require TPM hardware.

## Fedora installer checks

`tests/test_installer.py` verifies source-installer preflight, staged file/model
integrity, path/link rejection and rollback. `packaging/installer/test-install.py`
is an explicit root-only private-namespace file-install test; it requires a
prepared private stage and already installed KDE/TPM policies, and never uses
real biometric profiles or edits PAM. See [installer guide](../docs/install-fedora.md).
