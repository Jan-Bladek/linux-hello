# Executed validation — 2026-10-10

This report distinguishes software checks from physical biometric, desktop and
Fedora validation. **This is not a production authentication release.**

## Environment

- Windows x86-64 host; Rust/Cargo 1.99.0, MSVC target for portable checks.
- Ubuntu in WSL2, x86-64 Linux 6.18.33.2-microsoft-standard-WSL2.
- Rust 1.99.0, Linux GNU target; native C/Clang, font and Wayland development
  libraries installed in WSL for compilation. The toolchain/cache/build products
  were kept outside the delivered repository.
- No V4L2 video nodes were available: the release CLI returned `[]` from `cameras`.
- No host PAM/authselect configuration was edited. No privileged Linux Hello
  service was enabled or started. A temporary, unprivileged session-bus daemon
  was started for the executable smoke test and shut down with SIGTERM.

## Results

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| Linux `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| Windows Clippy for all non-GUI workspace crates/targets | Passed |
| Linux `cargo test --workspace --locked` | 43 passed; physical camera and two isolated-bus tests ignored in this invocation |
| Isolated session-bus tests, explicitly including the two ignored IPC tests | 2 passed; physical camera test not selected |
| Windows portable camera/core/face/storage/IPC/PAM/CLI tests | 35 passed |
| Linux `cargo build --workspace --release --locked` | Passed, including CLI, daemon and Slint GUI |
| Release CLI `--version`, `security`, `cameras` | Passed; security flags false, no video devices |
| Release daemon + CLI `status --session` on disposable D-Bus | Passed; protocol v1, diagnostic-only, UID resolved through the bus; clean SIGTERM exit |
| `cargo deny --locked check licenses sources bans` | Passed; duplicate-version warnings remain informational |
| `cargo audit` | Zero known vulnerability entries; two unmaintained-dependency advisories |
| `cargo audit --deny warnings` | **Failed as intended:** RUSTSEC-2025-0141 and RUSTSEC-2026-0192; no suppressions |
| D-Bus and inactive Polkit XML parsing; packaging shell syntax | Passed |
| `systemd-analyze verify` using a scratch copy pointing to the built daemon | Passed; WSL mount executable/world-writable mode warnings on the scratch file |
| Fedora RPM build/install/removal, SELinux, system-service execution | Not performed |

Linux has **45 passing tests** across the normal and explicit isolated-bus runs.
Windows exercises an overlapping portable subset, not 35 additional independent
security properties. The physical-camera test remains intentionally unexecuted.

The service verifier used the real service directives with only `ExecStart`
redirected to the local release binary, without installing anything under
`/usr/libexec` or starting the unit. It does not validate the service sandbox at
runtime, Fedora system-bus policy, SELinux policy, or RPM scriptlets.

## GUI verification

The compiled Slint interface was instantiated and rendered using Slint's headless
software window. All five tabs rendered at 980×740 and the minimum 760×660 size;
the resulting images were visually inspected. The [overview screenshot](screenshots/overview.png)
is from that executable renderer test, not a design mockup. It shows the actual
camera-off state, with no invented device or authentication status.

This confirms rendering and layout initialization, not interactive operation on
GNOME, KDE Plasma or Hyprland. Live camera preview, desktop theme propagation,
keyboard/screen-reader behavior and compositor-specific behavior still need
physical Fedora testing. Test-only screenshot export writes UI images, never
camera frames.

## What these checks establish

Tests cover bounded and malformed frames; format conversion; asynchronous queue
limits; cancellation, drop and reconnect identity; caller/context checks; event
ordering, challenge binding and expiry; unavailable PAD; numeric/quality guards;
synthetic enrollment/matching; authenticated encryption; tampering and wrong keys;
owner binding; Linux symlink/hard-link rejection; atomic save/load/delete; CLI
failure exit codes; real D-Bus transport and caller UID; concurrent diagnostic
requests; and a test-only fprintd protocol peer.

No synthetic biometric response is used to authenticate a real user. No raw
biometric sample or key was written into diagnostic reports. The missing inference
and PAD implementations are explicit error/disabled states, not simulated
production behavior.

## Remaining gates

1. Run the documented hardware qualification on the actual Lenovo and record
   device/driver/format/emitter behavior, unplug/replug and cancellation results.
2. Select and pin properly licensed detector/recognizer weights, implement ONNX
   inference/alignment, and evaluate accuracy, latency, memory and IR suitability.
3. Implement and independently evaluate PAD and the trusted acquisition path.
4. Review key provisioning, storage and revocation; evaluate TPM protection.
5. Implement trusted enrollment, privileged IPC authorization and the narrow PAM
   adapter; validate password fallback and recovery only in a disposable Fedora VM.
6. Resolve the two maintenance advisories and complete dependency/source notices.
7. Validate Fedora RPM lifecycle, SELinux, service hardening and supported desktops.

No accuracy, FAR/FRR, liveness, secure-erasure, hardware-backed trust or public
stable-release claim follows from these automated checks.
