# Upstream research and dependency decisions

Reviewed 2026-10-10; optional OpenCV adapter added 2026-10-11. Exact direct Rust versions are in the workspace `Cargo.toml`;
the committed `Cargo.lock` fixes the resolved graph. Research establishes API and
licensing direction, not hardware compatibility or security certification.
The initial graph has unresolved upstream maintenance advisories, documented in
[dependency status](dependencies.md); current stable direct versions do not imply
that every transitive dependency remains maintained.

## Version baseline

| Item | Selected version / disposition |
|---|---|
| Rust toolchain / edition | 1.99.0 / 2024 (`rust-toolchain.toml`) |
| zbus | 5.19.0, Tokio backend |
| Slint / slint-build | 1.18.1 / 1.18.1, winit backend and software renderer |
| V4L wrapper | v4l 0.14.0 |
| Tokio | 1.53.2 |
| serde / serde_json | 1.0.229 / 1.0.151 |
| thiserror / clap | 2.0.21 / 4.6.7 |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 |
| chacha20poly1305 / zeroize | 0.11.0 / 1.9.1 |
| getrandom / sha2 | 0.4.3 / 0.11.0 |
| tempfile / libc | 3.27.0 / 0.2.190 |
| OpenCV Rust binding / native runtime | Optional `opencv` 0.101.1 / tested Fedora OpenCV 4.13.0; CPU-only local GUI and experimental verification |
| ONNX Runtime | Not used or bundled; ONNX inference uses optional OpenCV DNN |
| fprintd / PAM / Polkit / systemd | Fedora-provided system components; record installed RPM versions in hardware/VM reports |

## D-Bus, UI and camera

[zbus 5.19.0](https://docs.rs/zbus/5.19.0/zbus/attr.interface.html)
provides interface macros and message-header injection, including the sender unique
name. That transport mechanism must be combined with system-bus credentials and
explicit policy for future sensitive calls. Public diagnostics alone need no
identity claim from the caller.

[Slint 1.18.1](https://docs.rs/slint/1.18.1/slint/) exposes Rust components,
callbacks and image buffers suitable for a native preview. The selected software
renderer avoids bundling a large GPU renderer but still requires target-desktop
testing. No accessibility or Wayland compatibility certification is claimed.

The [kernel V4L2 API](https://docs.kernel.org/userspace-api/media/v4l/v4l2.html)
is the authority for capability negotiation, buffers and streaming semantics. The
Rust wrapper does not make a driver-reported IR label trustworthy. Lenovo marketing
specifications alone cannot identify the actual video node or emitter protocol;
capture a physical-device capability report before selecting one.

## Existing Linux projects

[Howdy](https://github.com/boltgolt/howdy) is a relevant existing Linux facial-PAM
project to study for device quirks and integration issues. This workspace does not
reuse its authentication decision as a security boundary or infer evaluated PAD
from a similar user experience.

[fprintd's manager API](https://fprint.freedesktop.org/fprintd-dev/Manager.html)
enumerates readers. The [device API](https://fprint.freedesktop.org/fprintd-dev/Device.html)
provides enrollment/status and claim/release operations governed by fprintd's own
Polkit policy. Future management should use those APIs and existing system prompts;
Linux Hello must not copy fingerprint templates or implement competing drivers.

## Licensing decisions

Original shared code is Apache-2.0. `hello-settings` selects GPL-3.0-only for the
combined GUI. Slint offers GPL, royalty-free and commercial routes; the project
uses GPL for the open-source GUI and must provide corresponding source and notices
when distributing the combined binary. The shared source files retain Apache-2.0.
The royalty-free route has separate attribution/platform terms and is not the
packaging choice here. See [Slint's license explanation](https://github.com/slint-ui/slint/blob/master/LICENSE.md).

RPM top-level license expression is `Apache-2.0 AND GPL-3.0-only`, with the GUI
subpackage declaring GPL-3.0-only. This identifies project licensing, not a complete
bill of materials. A distributable binary/SRPM additionally needs all applicable
transitive notices, corresponding sources and Fedora bundled-dependency review.
`cargo deny` checks SPDX expressions; it does not replace that review.

InsightFace code and pretrained weights have different terms. Its supplied weights
are restricted to noncommercial research, so permissively licensed library code
does not authorize unrestricted weight redistribution. No InsightFace model is
bundled. See [upstream model licensing](https://github.com/deepinsight/insightface/blob/master/python-package/README.md).

## ONNX model-selection and evaluation gate

The optional YuNet/SFace adapter now passes functional detection/alignment/inference
tests. Exact artifact pins, preprocessing, upstream licenses and unresolved training-data
provenance are recorded in [inference diagnostics](inference.md). No candidate has passed
both the full licensing review and security evaluation in this environment.
A detector and embedding model need explicit weight and training-data provenance,
redistribution terms, model hashes, opset/runtime compatibility, preprocessing,
alignment and normalization definitions. PAD needs a separate design and report.

Evaluate a local, consented test set on the actual IR/RGB sensor; record participant
and condition coverage, sample counts, threshold selection procedure, FAR/FRR with
uncertainty, latency distribution, resident memory, startup time and failure cases.
Hold threshold calibration data separate from evaluation. Include printed/display
replay, masks, injected/virtual camera input and device substitution. An IR image
alone is not a liveness experiment. No numerical results are invented here.

[ONNX Runtime's validation guidance](https://onnxruntime.ai/docs/) explains that
format validation does not establish application suitability and that malicious
models may exhaust compute/memory. Treat models as executable input: verified
provenance, bounded worker resources and isolation are required.

Current [upstream privacy documentation](https://github.com/microsoft/onnxruntime/blob/main/docs/Privacy.md)
describes telemetry in native builds, including non-Windows systems. A future
runtime must be built without telemetry, initialize with all relevant telemetry
controls disabled and pass a network-isolation test. On applicable versions set
`ORT_DISABLE_TELEMETRY=1` before initialization; a later API call alone may be too
late to prevent initialization activity. No ORT code runs in this preview.

## Polkit, PAM, authselect and TPM

[Polkit's authority API](https://polkit.pages.freedesktop.org/polkit/eggdbus-interface-org.freedesktop.PolicyKit1.Authority.html)
supports a `system-bus-name` subject. Future authorization must use the sender
resolved by the broker, rather than trusting a caller-supplied PID or name. The
reserved project policy denies every sensitive action and is not installed.

[authselect's upstream manual](https://github.com/authselect/authselect/blob/master/src/man/authselect.8.adoc)
documents custom profiles, backups, checks and profile inheritance. Modified PAM
templates must be copied, so the project must track upstream profile changes and
rebase/test its changes during updates; symlinking an edited PAM template would
modify the parent. No profile is selected by this package.

[systemd credentials](https://github.com/systemd/systemd/blob/main/docs/CREDENTIALS.md)
and [systemd-creds](https://github.com/systemd/systemd/blob/main/man/systemd-creds.xml)
are candidates for separating keys from encrypted templates, optionally using a
TPM. Deployment needs verified measured-boot policy, upgrade/recovery behavior and
relevant systemd-version testing. Sealing a key does not protect a running
compromised OS or attest a camera/PAD path. Optional [TPM key wrapping](tpm.md) is implemented using systemd-creds; measured-boot and recovery qualification remain open.

## Evidence still required

The recorded Fedora KDE camera/IR, enforcing SELinux and owner-confirmed sudo,
login and unlock results are summarized in [validation](validation.md). Other
hardware/desktops, representative NIR/PAD evaluation, independent storage and
privilege review, broad PAM VM coverage, RPM lifecycle and upgrade/recovery drills
remain release blockers. A successful build cannot fill them in.
