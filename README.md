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
| `hello-camera` | V4L2 enumeration, capability/format/size inspection, IR hints, on-demand MMAP capture, bounded asynchronous queue, cancellation, same-device reconnect, GREY/RGB24/YUYV preview; basic Lenovo YUYV/GREY capture and reopen tested | Long-duration hardware stability, IR illumination, multi-planar/compressed formats, other Fedora devices |
| `hello-cli` | JSON device diagnostics, bounded capture, tested SIGINT/reopen, Lenovo qualification probe, optional isolated face probe, quality/control inspection and model evaluation, D-Bus status, security report, fprintd status | Full Lenovo shutter/emitter/suspend/desktop qualification |
| `hello-settings` | Native Slint overview, device selection, actual local preview, optional isolated face check, session/multi-sample references and encrypted persistence, stop/timeout, diagnostics, security information and daemon connectivity | Fedora GNOME/KDE/Hyprland usability and accessibility testing |
| `hello-core` | Explicit security status and one-shot diagnostic verification state machine | Privileged authorization protocol and evaluated PAD evidence |
| `hello-face` | Bounded quality statistics, normalized embeddings, multi-sample template aggregation, model-bound matching, validated pipeline and optional CPU YuNet/SFace detection/alignment/ONNX inference | Model/data provenance review, accuracy/latency/memory/IR/PAD evaluation |
| `hello-storage` | XChaCha20-Poly1305 authenticated envelopes, owner binding, zeroizing secret values, Linux descriptor-relative atomic save/load/delete | Trusted key provisioning, security review and TPM integration; GUI diagnostic references use a separate passphrase |
| `hello-daemon` / `hello-ipc` | Real read-only D-Bus service, bus-derived requester UID, bounded client calls, concurrency tests | Trusted enrollment and authentication; camera frames never transit this service |
| `hello-fingerprint` | Real fprintd reader discovery and caller's enrolled-finger status | Fingerprint enrollment/removal UI; use existing fprintd tools meanwhile |
| `hello-pam` | Readiness/blocker report only | No `pam_sm_*` exports and no installable module |
| Packaging / CI | RPM specification, hardened manual-start diagnostic service, D-Bus policy, release gates and CI | Fedora RPM install/remove, SELinux and disposable-VM validation |

The camera backend supports single-planar streaming capture devices. Unsupported
devices remain visible with an explanation. A monochrome format or an "IR" name
is a hint, never liveness evidence. Camera capture starts only on an explicit user action and stops after that operation.

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
Optional local YuNet/SFace inference is documented in [inference diagnostics](docs/inference.md);
models are fetched only by an explicit utility command, and are never bundled.
The optional CLI and its `hello-face-worker` must remain side by side. See
[IR results](docs/ir-qualification.md) and [model review/evaluation](docs/model-evaluation.md).

Read-only vendor-control inspection is available through
`timeout 30s python3 tools/inspect-uvc-controls.py --device /dev/video2`.
It reports declared UVC extension-unit lengths and support flags without capture
or control writes. Scoped IR mode is now available using
`hello capture --device /dev/video2 --frames 90 --timeout 15 --ir-emitter` on the
tested Chicony firmware, with verified restoration when capture ends.

Before running model evaluation, use `hello face-dataset-check --manifest PATH`
with the optional face build to validate all dataset files and session/split metadata.
It produces aggregate diagnostics without loading models or performing inference.
The GUI starts with its camera off. Select **Odśwież**, open **Osoba**, choose a
device, and select **Podgląd kamery**. **Stop**, the frame cap, or the timeout ends
capture. For the tested IR camera, select **Doświetlenie podczerwienią (IR)** before starting;
its prior mode is restored when preview ends. Unsupported modules return an error.
Refresh queries fprintd and daemon diagnostics independently of capture.

For the optional GUI face check, build the GUI and its sibling worker together:

```sh
cargo build --locked --release -p hello-cli -p hello-settings --features hello-settings/face-diagnostics
LINUX_HELLO_MODELS="$PWD/work/inference/models" target/release/hello-settings
```

With explicitly fetched pinned models available, choose **Odśwież**, open **Osoba**
(previously Camera), select the camera and optionally IR illumination. Stop preview
before adding/recognizing. Model directory and Check face / Compare 3 samples /
Remember face are under **Ustawienia → Diagnostyka kamery i modeli**. A default GUI
build does not expose optional face controls. Capture initializes its isolated
worker before the camera. Person scanning and recognition keep one bounded camera,
IR lease and loaded worker throughout the operation, then join capture, restore
IR and reap the worker before publishing a result. Legacy single-probe diagnostics
release the camera before inference.
Stop or closing the window cancels and awaits cleanup.

The current accuracy refactor, v2 templates, IR quality findings and measured
functional performance are documented in [accuracy improvements](docs/accuracy-improvements.md).
New enrollments begin with three frontal anchors before pose variation. Old v1
references remain usable with their original scoring; re-enroll to obtain v2.

To scan, label and recognize a face:

1. In **Osoba**, choose **Skanuj nową twarz**. One explicit click starts up to eight
   quality-accepted captures. Follow each position instruction; a three-second pause
   at each new position gives time to move. The scanner shows acquired camera frames,
   a positioning oval, an animated scan line and accepted-sample progress.
2. After all eight captures, enter the person's name and choose **Podpisz twarz**.
   Names allow 1–64 characters and at most 256 UTF-8 bytes.
3. Choose **Skanuj i rozpoznaj** and look at the camera. Three fresh, suitable
   frames must all match the saved person; no head-turn instructions are required.
   The result shows **Rozpoznano: [name]** only after all three pass. Camera/IR mode
   must match the reference. The separate **Test ruchu głowy (diagnostyka)** in
   Settings retains the optional five-pose experiment.
4. **Stop** cancels the active capture/pause. **Kontynuuj skan** resumes accepted
   samples after rejection or cancellation. **Anuluj dodawanie** discards the new
   set and restores the previous person, if present.

The scanner oval is a positioning guide, not measured face bounds or liveness.
The preview follows a continuous bounded camera stream throughout person scanning
and recognition. Analysis takes successive fresh frames from a single latest-frame
buffer; it does not keep an image history.
The displayed frame is cleared when the job ends; no image is saved. Enrollment
permits up to eight quality attempts within 15 seconds per position, subject to
the session time and frame budgets.

The GUI holds one person reference. **Dodaj inną twarz** scans its replacement;
the previous reference stays intact until eight accepted samples finish. The new
face must be labelled before GUI recognition. **Ustawienia → Nazwa twarzy** renames
without recapture. Names are local labels, not verified identities/system accounts;
control/bidi formatting characters and oversized names are rejected. Names are
not added to CLI evaluation reports.

To retain the person after restart:

1. Choose **Zapisz / wczytaj** or open **Ustawienia**.
2. Enter a separate reference passphrase of at least 12 characters twice, then
   **Zapisz osobę**. Name and biometric template are encrypted together.
3. After restart, enter the passphrase in the first field and choose **Wczytaj osobę**.
   Return to Osoba and select the original camera/IR mode.
4. After renaming, save again to persist the name. **Usuń z pamięci** affects memory;
   **Usuń zapisany plik** removes the encrypted reference and clears memory.

Storage remains one authenticated encrypted template and public salt in
`$XDG_DATA_HOME/linux-hello/diagnostic-reference`, or
`$HOME/.local/share/linux-hello/diagnostic-reference`. No photograph, plaintext
name sidecar, key, passphrase, raw embedding file or score history is saved.
Persistence is explicit; startup opens neither camera nor saved reference.
Wrong passphrases cannot overwrite an existing file. Forgotten passphrases require
removal and a new reference. Old templates remain readable for comparison, but
strict recognition requires eight samples containing landmark geometry and
individual exemplars. V1 references with these fields remain supported; V2
requires a new scan to obtain frontal anchors and pose-aware scoring. No automatic upgrade or deletion of old references occurs.

Ustawienia → **Experimental recognition** adjusts cosine threshold in [0.80, 1].
Default **0.80** is a conservative provisional policy, not camera calibration.
Each accepted acquisition must meet both centroid and exemplar median similarity.
V2 selects three references by nearest measured pose before scoring; V1 retains
the original median over all references. A nearest
stored shape must also satisfy six normalized landmark proportions (mouth/eye
width, eye-to-nose, eye-to-mouth, nose-to-mouth, eye/face width, face aspect ratio).
Geometry has provisional tolerances; five points are not a dense 3D shape model.
Frontal identity failures reject the attempt. During each turn at most two
below-threshold/shape-mismatch frames may be discarded and retried; a third
rejects the attempt. Neither discarded frame supplies owner or movement evidence.
Both stable frames confirming each turn must pass the original identity/shape
gates. Every owner-validated observation contributes to the final minimum, so a
stricter user threshold cannot be bypassed by choosing only a high median. Wrong pose or transient image quality is retried up to 16 times per stage,
within a 15-second stage budget and a bounded fresh-frame search. Stop remains available during
the three-second pose pause, 0.8-second retry pause and acquisition. The whole operation
is also limited by camera/worker time and frame budgets. Ordinary diagnostic
comparison remains three captures and does not produce a named recognition result.

Normal recognition collects three suitable, distinct owner-matching frames while
you look at the camera. It does not claim a motion challenge or validated PAD.
Stored V1/V2 references remain compatible. The following baseline/turn rules apply
only to the optional diagnostic motion test.

The first stage establishes your natural baseline from two stable owner-matching
captures within five seconds, allowing brief unusable IR frames between them;
hold your head still rather than turn. The scanner shows measured turn
progress only during turns, warns about the wrong direction and distinguishes
quality rejection from movement measurement.
IR captures warm up once and skip dark frames without requiring consecutive
frames to have equal brightness. Quality is measured on the detected face, so
a dark IR background does not reject an otherwise readable face. See the
[Howdy source analysis and acquisition changes](docs/howdy-analysis.md).
The randomized motion check measures nose displacement along the eye axis,
normalized by eye separation, and requires two opposite turns and return to the
starting pose. Exact repeated frames and changed references abort the check.
It can reject a stationary image or rigid translation/in-plane rotation, but
**is not evaluated presentation attack detection (PAD)**: replay video, masks,
projective movement of a print/display and injected video remain concerns. No
blink detection, depth sensor, dense mesh or evaluated RGB/IR PAD model is present.
Do not treat an IR emitter as proof of liveness. Recognition remains experimental;
privileged system login remains disabled. [NIST PAD evaluation](https://pages.nist.gov/frvt/html/frvt_pad.html)
describes why spoof resistance requires separate evaluation. Test saved-person
and consented other-person trials on the actual camera before judging reliability.

Ustawienia → **Manual recognition trials** provides user-labelled saved-person and
consented other-person trials. Counts separate recognition, nonrecognition,
ambiguity and acquisition failure; cancellation does not count. At most 1000
outcomes stay in memory until Clear trials or window close. Changing the reference
(including its name) or threshold requires Clear trials before new acquisition.
These are informal counts, not FAR/FRR estimates or a calibration dataset.

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

To continue on a physical Linux desktop, run `python3 tools/qualify-linux.py`
for a recorded preflight, then `python3 tools/qualify-linux.py --verify` for
software checks. Add `--device /dev/videoN` only after selecting the intended
camera to run bounded capture/release/reopen. See the
[qualification procedure](docs/hardware-support.md#lenovo-qualification-procedure).

## Architecture and project status

Start with the [architecture](docs/architecture.md), [threat model](docs/threat-model.md),
[authentication flow](docs/authentication-flow.md), and [biometric limitations](docs/biometrics.md).
[Upstream research](docs/research.md) records API and licensing decisions.
[Dependency status](docs/dependencies.md) records unresolved maintenance advisories;
the [research baseline](docs/research.md#version-baseline) lists exact direct versions.

Phase 1 and the Phase 2 software are implemented. Basic YUYV/GREY capture,
cancellation and reopen passed on the physical Lenovo; full hardware qualification
is pending. Selected non-privileged Phase 3–5 foundations are implemented; face
enrollment, evaluated recognition, PAD, trusted authorization and Phase 6 PAM
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
