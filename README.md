# Linux Hello

A Rust foundation for private, local biometrics on Fedora, with a working V4L2
diagnostic CLI and Slint camera preview. **This is a development release. It cannot
enroll a face or authenticate a user. It is not a replacement for a password.**

The immediate milestone is usable camera diagnostics and an auditable security
foundation. No model weights, fake recognition results, PAM module, telemetry, or
automatic authentication configuration are included.

![Actual Slint interface, rendered by the headless software-renderer test](docs/screenshots/overview.png)

## What works

| Component | Implemented behavior | Remaining validation or work |
| --- | --- | --- |
| `hello-camera` | V4L2 enumeration, capability/format/size inspection, IR hints, on-demand MMAP capture, bounded asynchronous queue, cancellation, same-device reconnect, GREY/RGB24/YUYV preview | Real cameras, IR illumination, multi-planar/compressed formats, Fedora device permissions |
| `hello-cli` | JSON device diagnostics, bounded capture, Lenovo qualification probe, D-Bus status, security report, fprintd status | Physical Lenovo Yoga Slim 7 14IMH9 qualification |
| `hello-settings` | Native Slint overview, device selection, actual local preview, stop/timeout, diagnostics, security information and daemon connectivity | Fedora GNOME/KDE/Hyprland usability and accessibility testing |
| `hello-core` | Explicit security status and one-shot diagnostic verification state machine | Privileged authorization protocol and evaluated PAD evidence |
| `hello-face` | Bounded quality statistics, normalized embeddings, multi-sample template aggregation, model-bound matching, replaceable engine trait | Actual ONNX inference, licensed weights, face detection/alignment, accuracy/latency/PAD evaluation |
| `hello-storage` | XChaCha20-Poly1305 authenticated envelopes, owner binding, zeroizing secret values, Linux descriptor-relative atomic save/load/delete | Key provisioning, security review, TPM integration; not connected to enrollment |
| `hello-daemon` / `hello-ipc` | Real read-only D-Bus service, bus-derived requester UID, bounded client calls, concurrency tests | Trusted enrollment and authentication; camera frames never transit this service |
| `hello-fingerprint` | Real fprintd reader discovery and caller's enrolled-finger status | Fingerprint enrollment/removal UI; use existing fprintd tools meanwhile |
| `hello-pam` | Readiness/blocker report only | No `pam_sm_*` exports and no installable module |
| Packaging / CI | RPM specification, hardened manual-start diagnostic service, D-Bus policy, release gates and CI | Fedora RPM install/remove, SELinux and disposable-VM validation |

The camera backend supports single-planar streaming capture devices. Unsupported
devices remain visible with an explanation. A monochrome format or an "IR" name
is a hint, never liveness evidence. There is no continuous camera capture.

## Build and run on Fedora

Use the exact Rust toolchain in `rust-toolchain.toml` and dependency graph in
`Cargo.lock`. See [installation](docs/installation.md) for Fedora build dependencies
and packaging. Ordinary diagnostics need your session's existing camera access,
not root.

```sh
cargo build --locked --workspace
cargo run --locked -p hello-cli -- cameras
cargo run --locked -p hello-cli -- security
cargo run --locked -p hello-cli -- capture --device /dev/video0 --frames 10 --timeout 10
cargo run --locked -p hello-cli -- probe --device /dev/video0
cargo run --locked -p hello-cli -- fingerprints
cargo run --locked -p hello-settings
```

Choose the device path from `cameras`; `/dev/video0` above is only an example.
Capture and probe report metadata and discard pixels. They create no image files.
The GUI starts with its camera off. Select **Refresh**, open **Camera**, choose a
device, and select **Start preview**. **Stop**, the frame cap, or the timeout ends
capture. Refresh queries fprintd and daemon diagnostics independently of capture.

For development IPC without installing or enabling a service, run inside an
isolated session bus:

```sh
dbus-run-session -- bash
# In that shell:
cargo run --locked -p hello-daemon -- --session &
cargo run --locked -p hello-cli -- status --session
# Stop the background daemon before leaving the shell.
```

System-bus installation is optional and has no authority to authenticate in this
version. A session-bus daemon is explicitly diagnostic, never an authentication
authority. No system authentication files are changed by these commands.

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
bash tests/integration/run-session.sh
cargo audit
cargo deny --locked check advisories licenses sources
cargo build --locked --workspace --release
```

The hardware test is ignored by default and must be explicitly run with a real
device. Read [hardware support](docs/hardware-support.md) before testing. Automated
camera and biometric tests use synthetic inputs; they do not measure physical
attack resistance. The full test inventory and actual execution results are in
[validation](docs/validation.md).

## Architecture and project status

Start with the [architecture](docs/architecture.md), [threat model](docs/threat-model.md),
[authentication flow](docs/authentication-flow.md), and [biometric limitations](docs/biometrics.md).
[Upstream research](docs/research.md) records API and licensing decisions.
[Dependency status](docs/dependencies.md) records unresolved maintenance advisories;
the [research baseline](docs/research.md#version-baseline) lists exact direct versions.

Phase 1 and the Phase 2 software are implemented. Physical camera qualification is
pending. Selected non-privileged Phase 3–5 foundations are implemented; face
enrollment, recognition inference, PAD, trusted authorization and Phase 6 PAM
integration remain incomplete. Phase 7 release gates have not been met.

No production security claims or biometric accuracy statistics are made.
Installation never enables PAM, changes disk encryption, or silently changes
login configuration. See [SECURITY.md](SECURITY.md) and the recovery instructions
before any future authentication integration.

## Licensing

Shared crates are Apache-2.0 ([LICENSE](LICENSE)). The Slint settings application
is GPL-3.0-only ([LICENSE-GPL-3.0](LICENSE-GPL-3.0)); this project chooses Slint's
GPL licensing option. Distributing GUI binaries requires corresponding source
and compliance with GPL-3.0. No pretrained model license is implied by the code
license, and no weights are distributed.
