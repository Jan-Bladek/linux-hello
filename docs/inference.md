# Face inference, enrollment and verification

Current pipeline: YuNet + SFace + OpenCV DNN on CPU. Optional features are off
by default. `hello-cli/face-opencv`, `hello-settings/face-diagnostics` and the
optional PAM verifier use the same pinned worker pipeline. The diagnostic D-Bus
daemon performs no inference and has no authentication authority. Successful
face detection is not identity verification or validated PAD. See [scope](index.md).

## Build and models

```sh
sudo dnf install --setopt=install_weak_deps=False opencv-devel clang-devel
# Remaining commands run as the ordinary desktop user from the repository root.
python3 tools/fetch-face-models.py --output work/inference/models
cargo build --locked --release -p hello-cli -p hello-settings \
  --features hello-settings/face-diagnostics
LINUX_HELLO_MODELS="$PWD/work/inference/models" target/release/hello-settings
# Choose the actual device from `hello cameras`:
target/release/hello face-probe --device /dev/video2 \
  --models work/inference/models --ir-emitter
```

The downloader requires a new output directory. It verifies byte lengths and
hashes and records upstream notices. A failed download may leave an incomplete
directory: inspect it and choose a new directory before retrying. No weights
are fetched during build, tests, startup or inference, or bundled in the RPM.
Keep `hello-face-worker` adjacent to optional CLI/GUI binaries. Native backend
contracts and pinned artifacts must agree; mismatched worker readiness fails
before capture. `/dev/video2` and emitter control above are tested-host examples,
not universal camera configuration. See [hardware](hardware-support.md).

## Artifact and preprocessing contract

Pinned [OpenCV Zoo commit](https://github.com/opencv/opencv_zoo/tree/47534e27c9851bb1128ccc0102f1145e27f23f98):

| Artifact | Bytes | SHA-256 | Directory license |
|---|---:|---|---|
| `face_detection_yunet_2023mar.onnx` | 232589 | `8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4` | MIT |
| `face_recognition_sface_2021dec.onnx` | 38696353 | `0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79` | Apache-2.0 |

The file hashes and lengths match upstream Git LFS pointers. Upstream directory
licenses are evidence for this evaluation, not a completed rights/provenance
review of training data. [Upstream issue 313](https://github.com/opencv/opencv_zoo/issues/313)
raises the unresolved mapping between SFace weights and training datasets.
Redistribution and production adoption require a separate review.

Linux opens each file with `O_NOFOLLOW|O_NONBLOCK`, checks the opened descriptor is
a regular file of the exact expected size, reads with a size cap, hashes those
bytes and passes those same bytes to OpenCV's in-memory model constructors. There
is no second pathname-based load, remote inference, automatic model discovery or
custom operator registration. Missing, modified or symlinked artifacts fail
before camera access. Parent directory names are not treated as trusted identity;
the pinned bytes establish artifact identity.

OpenCV 4.11 or newer in the 4.x series is required for the memory constructors;
4.13.0 was tested. Backend and target are explicitly OpenCV/CPU, with one OpenCV
thread. Source RGB/gray rows are validated, padding stripped and converted to BGR.
YuNet runs at the frame dimensions with presence threshold 0.75, NMS 0.3 and top-K 500.
Embedding extraction still requires exactly one face and confidence at least 0.90.
Detection bounds, five landmarks and confidence must be finite and in-frame.
Landmark order is right eye, left eye, nose, right mouth, left mouth. Exactly one
face and the existing diagnostic quality policy are required; failures are
reported, never replaced with synthetic embeddings.

SFace `alignCrop` produces 112×112 BGR samples. `AlignedFace` holds interleaved
integer-valued BGR floats in 0..255. `feature` converts these to RGB/NCHW; the
ONNX model contains its own normalization. No extra image scaling/normalization
is added. Output must be 128 finite, nonzero values; `Embedding` normalizes their
L2 norm and binds them to the exact recognizer digest. See the upstream
[alignment and feature implementation](https://github.com/opencv/opencv/blob/4.13.0/modules/objdetect/src/face_recognize.cpp).
The recognizer digest is checked along with dimensions, template version and
sensor/IR binding; it is not a cryptographic attestation of the full acquisition path.

## Infrared processing and quality

GREY is converted to the model's expected three-channel input without applying
RGB-model normalization twice. CLAHE may retry monochrome detection only;
alignment and SFace descriptors use the original pixels. A dark IR background
is assessed separately from the detected face. No exposure or unreviewed vendor
control is forced. The scoped IR lease supports only the whitelisted Chicony
firmware and restores its original setting with readback during cleanup.

| Gate | Current provisional policy |
| --- | --- |
| Face count/confidence | Exactly one presence candidate; descriptor confidence >= 0.90 |
| Usable size | Face at least 80×80, inter-eye distance at least 24 source pixels |
| Position | Coverage 0.04–0.65, edge clearance >= 0.015 |
| Face ROI exposure | Mean 0.08–0.92, contrast >= 0.03 |
| Detail | Edge energy >= 0.005 and edge/contrast >= 0.07 |
| Clipping | Overexposed fraction <= 0.12; combined dark/white fraction <= 0.60 |

These are acquisition heuristics, not calibrated optical sharpness, NIR accuracy
or spoof resistance. Native quality and geometry are checked before descriptor
generation. Multiple faces, invalid geometry or protocol data never supply a
usable verification frame. See [IR evidence](ir-qualification.md) and
[accuracy review](accuracy-improvements.md).

## Continuous capture and worker isolation

Person enrollment, passive verification and comparison use one loaded worker,
one bounded camera session and a single latest-frame buffer. Preview does not
retain a frame history. Samples use fresh frame digests; duplicate analyzed
frames and camera/reference changes abort. IR warmup skips 18 frames; RGB skips 8.
Transient darkness/no-face/quality errors may retry within the operation budget.

| Limit | Scope |
| --- | --- |
| Sample search | Up to 4 seconds and 4 model inferences |
| Live capture | 50-second operation budget,55-second camera timeout,3600 camera frames |
| Streaming worker | 60 seconds total,192 frames,20 CPU seconds |
| Worker transaction | 8-second initialization/request deadline |
| Worker resources | 1 GiB address space, bounded descriptors/file size, one OpenCV thread |
| Passive verification | 15 seconds and16 capture attempts, exactly3 accepted matching frames |
| PAM supervision | 25 seconds plus up to10 seconds cancellation grace; expired attempts reject |

The worker loads verified model bytes under a syscall allowlist, no-new-privileges,
no dumpability/core dumps and cleared environment. It cannot open new files,
network sockets, execute programs, spawn threads/processes or access a camera.
The parent sends bounded RGB frames through private pipes and validates bounded
replies (4096 bytes). There is no D-Bus image/embedding endpoint or temporary image.
Native parsing and memory are not an independently audited security boundary.

On normal completion/cancellation, capture joins, IR restoration is verified and
the worker is reaped before acceptance. Errors in cleanup reject the attempt.
SIGKILL, kernel hangs, unplug or power loss cannot guarantee restoration.
Legacy single-frame `face-probe` diagnostics close capture before inference;
they do not use the GUI's continuous operation. See [Howdy analysis](howdy-analysis.md).

## GUI enrollment and named references

In **Osoba**, choose a camera/IR mode, stop preview and select **Skanuj nową twarz**.
A scan captures eight valid samples: front, slightly nearer front, slightly farther
front, moderate left/right, chin up/down and front. Three frontal anchors precede
pose variation. A three-second position pause allows movement. Enrollment checks
quality, distinct samples, geometry and consistency; rejected samples are not added.
The positioning oval and scan animation are guides, not measured depth or liveness.

Accepted samples are preserved after **Stop**; **Kontynuuj skan** resumes, while
**Anuluj dodawanie** restores the previous reference. Completing the set replaces
one GUI reference; it does not enroll a gallery. A scan/local-only save does not modify the protected
PAM copy; unified system setup does so only after authorization and live checks.
Enter a name and choose **Podpisz twarz**. Names are local labels, 1–64 characters
and at most 256 UTF-8 bytes, with control/bidi formatting characters rejected.
Rename does not require recapture; save again to persist a change.

New native enrollments create V2 templates. Full eight-sample V1 templates remain
supported with their original scoring. V2 selects three exemplars by measured
pose before identity scoring and uses three frontal enrollment anchors. Unknown,
corrupt or incompatible model templates reject. Legacy partial references remain
readable for comparison but cannot perform strict verification. Re-enroll for V2
benefits or incompatible model changes; passive recognition alone needs no migration.

## Passive verification and diagnostics

**Skanuj i rozpoznaj** compares only with the selected reference. Look straight;
no head-turn calibration/challenge is required. Exactly three distinct suitable
frames must all pass the configured threshold, centroid/exemplar and shape gates.
The minimum across accepted frames determines the result. V2 uses the median of
three pose-selected exemplars plus the full centroid; V1 uses its original median.
Six normalized five-landmark shape ratios provide a provisional compatibility
check, not a dense mesh, depth sensor or PAD.

The threshold is adjustable upward in the GUI from the provisional default/floor
0.80 to 1. Identity mismatch rejects immediately. Weak-quality/no-face/transient
capture may retry within 15 seconds/16 attempts; multiple faces, duplicates,
changed reference, model/camera errors and cancellation abort. Partial results
cannot be recognized, and all cleanup must finish before the displayed result.

Settings retains **Check face**, single-reference comparison, **Compare 3 samples**,
and **Test ruchu głowy (diagnostyka)**. Comparison-only results are mathematical
scores. The optional five-stage motion experiment is separate from normal
verification and is not validated PAD. Its earlier development is archived in
[inference history](inference-history.md).

Manual recognition trials allow user-labelled saved-person/consented other-person
attempts. Counters are bounded to 1000 outcomes in memory; reference/threshold
changes require a reset. Cancellation is not counted. These declarations and
aggregate counts are not an independent FAR/FRR study or calibration dataset.

## Multi-sample encrypted diagnostic reference

After naming, Settings offers application cards and **Zapisz i skonfiguruj**.
This saves the GUI reference and performs [unified system setup](kde-login.md#unified-gui-setup).
Developer mode retains the local-only save and inference controls. Use
**Wczytaj osobę** in Settings to load the GUI reference. Saving requires
a separate passphrase of at least 12 Unicode characters, at most 1024 UTF-8 bytes,
entered twice. Loading uses the first field. Input fields are cleared when work
starts; keys/passphrase buffers are zeroizing, but Slint shared strings are not
guaranteed wiped. The reference is not loaded automatically at startup.

The path is `$XDG_DATA_HOME/linux-hello/diagnostic-reference`, otherwise
`$HOME/.local/share/linux-hello/diagnostic-reference`. Private directories are 0700;
`template.lht` and `passphrase.salt` are 0600, owned, bounded regular single-link
files accessed without symlink traversal. Salt consists of `LHSALT01` plus 16 random
public bytes. Argon2id v19 uses 64 MiB, 3 passes, 1 lane and a 32-byte output key.
XChaCha20-Poly1305 authenticates the UID-bound envelope with a fresh nonce.
The template includes name, model-compatible exemplars, centroid, geometry and
sensor/IR binding. No raw image, plaintext name sidecar or embedding file is saved.

An advisory directory lock spans salt/decrypt/save/delete. Saving over an existing
reference first authenticates it; wrong passphrases preserve its bytes. Missing
salt beside ciphertext fails. Delete removes the template and clears memory while
retaining the public salt. Physical erasure, anti-rollback, GUI-store TPM protection, swap
exclusion and protection against root/same-UID compromise are not guaranteed.
A forgotten passphrase requires deletion and a new reference.

PAM uses a separate protected import, random root-owned key and account binding;
GUI storage does not itself grant OS access. See [sudo provisioning](sudo-integration.md).

## Reports and evaluation

Metadata reports include selected format, detection confidence, quality, inference
latency/CPU/RSS, embedding status, threshold, score and failure reason. They omit
raw pixels, embeddings and private landmark geometry. GUI/diagnostic readiness
flags remain false because the general security-qualified protocol is unavailable;
separately opted-in PAM adapters can still authenticate experimentally.

`face-dataset-check`, `face-evaluate` manifest V1/V2 and `face-benchmark --stream`
are documented in [model evaluation](model-evaluation.md) and
[accuracy improvements](accuracy-improvements.md). No representative NIR FAR/FRR,
validated PAD or Windows Hello-equivalent protection is established.

## Functional integration test

The ignored test accepts an explicitly supplied **public** 512×512 raw RGB fixture
and models. It does not acquire a camera or download anything. For the recorded
run, OpenCV's `samples/data/lena.jpg` from tag 4.13.0 was converted using ImageMagick:

```sh
curl --fail --location https://raw.githubusercontent.com/opencv/opencv/4.13.0/samples/data/lena.jpg -o work/inference/upstream-lena.jpg
# Verify the public JPEG digest before using it:
echo '7de7ed51a1594fff247f4cae2301eceacf5313d6011e37b4a4c8733f7bb72c07  work/inference/upstream-lena.jpg' | sha256sum --check
magick work/inference/upstream-lena.jpg -depth 8 rgb:work/inference/upstream-lena.rgb
LINUX_HELLO_MODELS="$PWD/work/inference/models" \
LINUX_HELLO_TEST_RGB="$PWD/work/inference/upstream-lena.rgb" \
cargo test --locked -p hello-face --features opencv --test opencv_inference -- --ignored --nocapture
```

The test checks one face, 128 dimensions, repeated-input similarity >0.99999,
model binding, rejected non-finite/out-of-range/fractional aligned input, no face
on a blank image, and two-face rejection on a duplicated public fixture. This is
software functionality evidence only; it supplies no biometric accuracy estimate.
No public fixture is bundled in the project.

## Isolated worker functional test

```sh
LINUX_HELLO_MODELS="$PWD/work/inference/models" \
LINUX_HELLO_TEST_RGB="$PWD/work/inference/upstream-lena.rgb" \
cargo test --locked -p hello-cli --features face-opencv --test worker real_ -- --ignored --nocapture
```

The direct `hello-face` model test above verifies adapter math in-process. This
second explicit test verifies the actual supervised syscall/resource boundary.
Ordinary `cargo test` does not select either real-model test or acquire a camera.
