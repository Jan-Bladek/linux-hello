# Historical inference development notes

This archived log records superseded behavior during 2026-10-11–12. It is not
an installation guide or a description of current authentication. Earlier limits,
thresholds, UI labels and disabled-PAM statements apply only to their milestone.
Use [current inference](inference.md), [scope](index.md) and
[validation](validation.md) for the current implementation and evidence.


`hello-face/opencv` implements CPU YuNet detection, five-landmark SFace alignment
and 128-dimensional SFace embeddings. `hello-cli/face-opencv` adds `face-probe`.
These features are off by default. The daemon, GUI enrollment and PAM do not use
them; security readiness flags remain false. A passing probe is a functional
inference result, not an identity or liveness decision.

## Reproduce on Fedora

```sh
sudo dnf install --setopt=install_weak_deps=False opencv-devel clang-devel
# Run the remaining commands as your ordinary user, from the repository root.
mkdir -p work/inference
python3 tools/fetch-face-models.py --output work/inference/models
cargo build --locked -p hello-cli --features face-opencv
cargo test --locked -p hello-face -p hello-cli --features hello-cli/face-opencv
cargo clippy --locked -p hello-face -p hello-cli --all-targets --features hello-cli/face-opencv -- -D warnings
# Choose an exact device from `hello cameras`; this explicitly starts capture.
target/debug/hello face-probe --device /dev/video0 --models work/inference/models
```

The downloader requires a new output directory, fetches only explicit upstream
URLs, checks exact byte lengths and SHA-256, and records licenses, upstream
READMEs and provenance. A failed download can leave an incomplete directory;
inspect it and select a new directory before retrying. Downloads never run during
compilation, ordinary tests, application startup or inference. Weights are kept
under ignored `work/` and are not included in source or RPM distributions.

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
YuNet runs at the frame dimensions with score threshold 0.9, NMS 0.3 and top-K 500.
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
This digest does not identify all preprocessing policies; future persisted
production templates need an independently versioned whole-pipeline identity.

## Capture, cancellation and memory limits

The CLI spawns a separate `hello-face-worker` adjacent to its own executable;
it does not search PATH. The worker verifies artifacts before capture and installs
a Linux syscall allowlist **before native model parsing**. Only CPU/memory, timing,
signal and existing-pipe operations are permitted: new files, sockets, exec and
process/thread creation fail. The child starts with a cleared environment, one-thread
settings, no-new-privileges, no core dumps or dumpability, a 1 GiB address-space hard
limit, five CPU seconds for single probes (20 for bounded streaming sessions),
and file-descriptor/file-size limits. Unsupported sandbox
setup fails closed. The tested architecture is x86_64; aarch64 filter constants are
provided but were not tested on hardware.

The parent uses nonblocking private stdin/stdout pipes with bounded framing: one
RGB frame at most 1920×1080 and one JSON reply at most 4096 bytes. Embeddings cross
only these pipes, remain zeroizing objects, and are omitted from reports. The parent
validates model binding, dimensions, finite data and consistent status. There is no
D-Bus frame endpoint or temporary image file. stderr is discarded by the supervisor.

Eight-second wall deadlines cover initialization and each send/infer/reply/exit
transaction. On timeout, malformed/oversized output, crash or cancellation the parent
kills the worker's private process group and reaps it. A parent-death signal and
close-on-exec inherited descriptors reduce abandoned-worker exposure. Linux syscall
filtering is a process boundary, not protection against kernel vulnerabilities or a
compromised same-UID/root peer. The worker itself cannot acquire a camera.

After four warmup frames, the probe selects the first quality-acceptable candidate
within 30 delivered frames; otherwise it reports rejection of the last candidate.
For `--ir-emitter`, the tested Chicony MS face-mode control is scoped to capture
and restored before inference. The candidate cap is 90 to allow sensor settling
and alternating dark frames, with the same quality thresholds. See
[emitter control and hardware checks](ir-qualification.md).
This accommodates the measured monochrome settling delay without changing quality
policy. Camera acquisition retains its existing deadline and queue limits and is
closed before pixels are sent for inference. SIGINT during initialization, capture
or inference suppresses a passing report and releases/reaps resources. See the
[physical monochrome results](ir-qualification.md).

Reports contain dimensions, artifact hashes, elapsed time and rejection reason,
never image pixels, landmarks, embeddings or identity matches. Owned Rust RGB,
aligned and embedding buffers are zeroized. OpenCV's internal `Mat`/DNN allocations
and temporary copies are not guaranteed to be wiped, and this process does not
exclude swap/core dumps. There is no trusted camera acquisition or PAD. Gray/IR
model suitability and calibration have not been established.
Model review and the separate performance/FAR/FRR tools are in [model evaluation](model-evaluation.md).

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


## Optional GUI face check

Build `hello-cli` and `hello-settings` together with
`--features hello-settings/face-diagnostics`; keep `hello-face-worker` next to
`hello-settings`. The GUI uses the same bounded diagnostic as `hello face-probe`,
including pinned models, private pipes and worker limits. Models must already be
present locally; no runtime download occurs. Set `LINUX_HELLO_MODELS` or enter the
local model directory in Camera. Select Refresh, an enumerated capture device, and
Check face. IR illumination is opt-in and restricted to the tested firmware.

Preview and face checks are mutually exclusive. The previously enumerated device
identity must still match before opening its stream or enabling IR. Stop sets the
shared cancellation flag. Window closure waits for the active job to stop the
camera, restore scoped IR and reap the worker before runtime teardown. This does
not guarantee cleanup after SIGKILL, process crashes, USB/kernel hangs or power
loss. GUI results distinguish diagnostic success from enrollment/authentication;
face checks alone write no frame or embedding files; explicit Save encrypted can persist a diagnostic template.


## Session reference comparison

The optional GUI now offers Remember face / Replace reference and Clear memory.
Remember performs a fresh, explicit bounded capture through the same isolated
worker, then retains the normalized embedding in parent memory. Check face uses
a new acquisition to display cosine similarity to that reference. There is no
match threshold, acceptance flag, authentication token, or system enrollment.
One reference sample is not the library's multi-sample enrollment workflow.

`FaceProbe` has no Serialize or Debug implementation, and its embedding field
is private. CLI `face-probe` still returns metadata only and discards its embedding.
`SessionReference` rejects comparisons after a device-identity or IR-mode change;
Embedding also rejects changed model digests or dimensions. Failed acquisition
leaves the previous reference intact, while explicit replacement drops/zeroizes
it. Clear memory and normal window destruction drop/zeroize the reference. OpenCV's
internal native buffers retain the previously documented zeroization limitation.
Single-sample references are never saved. Explicit multi-sample persistence is described below; photographs, raw vector files and similarity history are never saved.

The score is a model calculation, not an evaluated identity or liveness verdict.
FAR/FRR, independent PAD, trusted multi-sample enrollment, key provisioning and
privileged authentication remain separate unfinished work.


## Multi-sample encrypted diagnostic reference

Camera → Add sample collects one fresh quality-checked sample per explicit click.
Eight distinct frame digests and compatible model embeddings produce a normalized
centroid using `hello-face::Enrollment`. Samples must share a camera/IR binding;
a provisional pairwise cosine coherence filter of 0.8 rejects inconsistent inputs.
This filter is not an evaluated verification threshold or proof that all samples
belong to one person. A failed sample preserves accepted samples and the previous
reference. Clear memory cancels sampling; Remember/Replace creates a single-sample
reference and resets pending enrollment. Saving is available only after finishing
eight samples. New unfinished samples block saving an older template.

Settings offers masked passphrase fields, Save encrypted, Unlock saved and Delete
saved. Saving requires matching passphrases of at least 12 Unicode characters
(maximum 1024 UTF-8 bytes). Unlock uses the first field only. Input fields are
cleared immediately when an operation starts, and backend passphrase/key/KDF
buffers use zeroizing memory. Slint's internal shared strings and text-input
buffers do not offer guaranteed wiping; masking and clearing do not remove that
limitation. No passphrase goes to logs, IPC, environment or a file. Storage/KDF
runs in a blocking job; window closure waits for it. Stop cancels capture, and is
disabled during encrypted storage operations.

The path is `$XDG_DATA_HOME/linux-hello/diagnostic-reference` or, by default,
`$HOME/.local/share/linux-hello/diagnostic-reference`. Explicit Save creates private
directories by descriptor-relative `mkdirat`/`openat` walks with no symlinks. The
final directory is owned by the current effective UID and forbids group/other
access. `template.lht` is an authenticated encrypted template; `passphrase.salt`
contains only an 8-byte `LHSALT01` format marker and a public random 16-byte salt.
Both files are 0600, bounded, regular, owned, and single-link on read. Existing
unsafe files/directories are rejected rather than chmod'ed. No files are created
at application startup or during Check face / Remember face / Add sample.

Argon2id v19 derives a 32-byte key with fixed 64 MiB / 3 passes / 1 lane; the salt
format marker defines those parameters and avoids untrusted cost parameters.
The RustCrypto implementation is pinned to 0.6.0; see its
[key derivation API](https://docs.rs/argon2/0.6.0/argon2/struct.Argon2.html#method.hash_password_into_with_memory).
The existing XChaCha20-Poly1305 envelope uses a fresh nonce per atomic save.
Its encrypted template includes an authenticated camera/IR binding; comparison
also checks model digest/dimensions. Moving USB topology or changing device
numbers may require collecting a new reference. This binding is not device
attestation. Unlock does not run the camera or make the template an identity token.

An exclusive advisory directory lock spans salt initialization, decryption and
save/delete for this passphrase workflow. Before replacing any existing template,
Save authenticates it with the supplied phrase; failure preserves its bytes.
A missing salt beside an existing template is rejected, never silently recreated.
Delete saved needs no passphrase, unlinks the template, fsyncs the directory and
clears memory. The public salt is retained for the next explicit save. Deletion
does not guarantee physical erasure from SSDs, journals, snapshots or backups.
No password recovery, password change, anti-rollback, TPM provisioning, or
protection from the same UID/root is claimed. Trusted enrollment and system
authentication remain disabled.


### Detailed eight-sample acquisition

The new capture target is eight, below the library cap of twelve. Instructions
cover front, slight left/right/chin up/down, slightly nearer/farther and a final
frontal view. These are instructions for the user, not automatic pose coverage
validation or liveness challenges. Eight successful captures produce a centroid;
reference matching still reports cosine similarity without a verification verdict.
Old encrypted three-sample references remain usable, and the GUI shows their actual
sample count on unlock. Starting a new set blocks saving until all eight succeed.

The isolated worker now measures the detected face region separately from the
whole-frame quality metrics. Bounds and landmarks are validated before cropping.
Quality measurements use the central 80% of each face-box dimension, excluding
its outer border to reduce contamination from detailed background/hair edges.
Gray crop buffers are bounded and zeroizing, remain private to the worker, and
are never stored. The reply contains only bounded aggregate quality statistics,
not face coordinates or landmarks. Parent validates the new metadata before use.

Enrollment additionally requires detection confidence ≥0.90, face-box dimensions
at least 96×96 pixels, image coverage 0.04–0.65 and normalized edge clearance
≥0.015. Central-face mean intensity must be 0.12–0.88; pixels outside intensity
0.02–0.98 must occupy at most 12% of the region. Contrast must be ≥0.04 and
neighboring-pixel edge energy ≥0.012. Edge energy is a detail/blur proxy, not a
calibrated optical sharpness measure. All cutoffs are provisional acquisition
policy, not measured accuracy/security thresholds. Rejections instruct the user
to move nearer/back/center, improve lighting/IR, or hold still/check the lens.
Rejected samples do not advance the counter or replace an existing reference.

These stricter checks apply to Add sample. Check face and single-sample Remember
retain their existing diagnostic behavior. User images were not acquired to tune
these cutoffs during implementation; live eight-sample usability still requires
explicit testing. FAR/FRR, pose coverage, illumination spectrum and PAD have not
been established by these changes.


### Repeated rejection correction

The original 0.95 acquisition detector cutoff was stricter than the 0.90 worker
embedding gate. The public pinned-model fixture produced confidence 0.9089786
with valid geometry and usable central-face detail, so the stricter cutoff
rejected a known successful diagnostic. Both gates now use the shared 0.90
constant. This is a diagnostic consistency fix, not an accuracy calibration.
The user's exact rejection reason has not yet been supplied; no claim is made
that every other quality/coherence rejection is resolved.

Add sample now retries at most three fresh bounded captures if face/sample
quality is unusable, stopping at the first suitable candidate. Camera/IR/worker
cleanup occurs per attempt; structural errors and cancellation are returned
immediately. Duplicate and enrollment-coherence rejection are never bypassed
or retried as accepted samples. Other quality thresholds are unchanged. An
unsuccessful third candidate returns its actual rejection. Each click adds at
most one accepted sample. Diagnostics shows the last metadata-only face report,
capture attempt count and UI result; it never includes vectors, raw images or
landmarks and does not automatically save this diagnostic history.


### Three-capture comparison series

Camera offers **Compare 3 samples** after remembering a reference or unlocking
an encrypted reference. It checks the actual reference and camera/IR binding
before opening any camera; an unfinished sample set alone is not a reference.
Three independently acquired probes use the same face-region quality checks as
Add sample, with at most three bounded acquisitions per probe (nine total).
Each attempt joins capture, restores the IR lease and reaps its isolated worker.
Repeated frame digests, rejected quality, camera/model mismatch, capture errors
and cancellation abort the series without a partial summary or reference update.

Only a complete series returns aggregate median/minimum/maximum cosine scores
and acquisition count. Scores remain signed in [-1, 1]; there is no verification
threshold, identity verdict, FAR/FRR estimate or liveness claim. The last report
replaces the Diagnostics pane, remains in memory, and contains no embeddings,
frame images or per-face coordinates. No reference or history is saved by this
action. Stop and window-close use the existing tracked-job cleanup.


### Earlier three-probe recognition policy (superseded)

The earlier **Recognize face** implementation compared three fresh quality-checked probes with the completed
eight-sample reference. Single-frame Remember face, unfinished sample sets and
legacy three-sample references cannot start recognition. The preflight checks
the actual in-memory reference, camera binding and threshold before acquisition.
Stored eight-sample references use the existing explicit passphrase unlock.

The bounded comparison returns a typed summary only after all three unique
probes succeed. A validated finite threshold in (0, 1] classifies **recognized**
if minimum cosine is >= threshold, **not recognized** if maximum is < threshold,
and **inconclusive** otherwise. Equality meets the threshold. Thus a high median
alone cannot hide a below-threshold probe. No face, multiple faces, poor quality,
duplicate frame, camera/model mismatch, worker error or cancellation is an
acquisition failure, not a different-person claim or a partial recognition.

Default threshold 0.363 follows the official OpenCV Zoo SFace demonstration:
https://github.com/opencv/opencv_zoo/blob/main/models/face_recognition_sface/sface.py
Upstream documents it for LFW, not this IR sensor or a multi-sample centroid:
https://docs.opencv.org/4.12.0/d0/dd4/tutorial_dnn_face.html
Settings allows explicit experimental changes; the threshold is not saved or
automatically lowered to make a face pass. No genuine/impostor dataset has been
used to calibrate this decision, so no local FAR/FRR claim is available.

Results are explicitly experimental and never authorize system authentication.
Diagnostics adds recognition_decision, recognition_threshold, experimental=true
and threshold_calibrated_for_camera=false to aggregate comparison metadata.
It retains authentication_ready=false and no biometric vectors/images. The
action does not change or persist the reference or append a score history.


### Manual recognition trials

Settings provides Test saved person / Test other person, Clear trials and Stop.
Users declare the expected label before capture; other participants must consent.
Originally each click used the three-probe quality/binding/worker pipeline and
records one aggregate outcome under the chosen label. Ordinary Recognize face
and Compare 3 samples do not change trial counters.

Counters separate recognized, not recognized, inconclusive and acquisition
failed for each label; cancellation changes neither counters nor trial context.
GUI commits counters and results together on its event thread, after capture
cleanup, so a pending Stop or window close does not count an unseen result.
Errors in camera, worker or quality count as acquisition failures, not mismatches.
Trial sets are bounded to 1000 outcomes and store only counters, numeric
threshold and a private reference generation, with no per-trial history.

Each successful reference change/load/clear/sample advances a process-unique
generation. The trial context requires the same generation and exact numeric
threshold. A mismatch is rejected before camera acquisition until Clear trials;
failed reference load does not change its generation. Save encrypted does not
change the active reference generation. Existing reference validation also
requires the original camera/IR mode and pinned model. Trial reports remain
labels_user_declared=true, calibrated_accuracy=false, authentication_ready=false.

The counters are an informal check on this sensor, not a calibration dataset,
independent evaluation or security evidence. Participants, sessions and capture
conditions are not verified or recorded. No rates/confidence intervals are
reported and no threshold is chosen automatically. Counts remain in memory
until reset/window close; no camera image, embedding or trial history is saved.


### Named person and guided GUI — 2026-10-11

The primary tab is now Osoba: camera/IR selection, a name field, guided eight-step
acquisition with accepted-sample progress and localized pose instructions, and a
separate recognition card. Start adds the first sample; subsequent captures still
require an explicit click. The head-position prompts remain guidance, not measured
pose or PAD. Hardware/model selection is disabled during a new enrollment.
Technical model controls and single-frame comparison tools moved to Ustawienia.

Starting a named set preserves the previous reference until all eight accepted
samples finish. Stop preserves pending progress; Anuluj dodawanie drops the new
set and restores the old reference. Rename requires no pending set and changes
only the local display label. Name changes advance reference generation, so old
manual trial sets require reset. Positive recognition shows Rozpoznano: [name];
negative/inconclusive/error states do not identify the target as the observed face.

Template format v1 has an optional display_name field, validated on deserialize
and assignment: trim outer spaces, 1–64 Unicode characters, maximum 256 UTF-8
bytes, no controls or bidi formatting controls. It is encrypted/authenticated
in template.lht with the biometric centroid, not placed in a sidecar or filename.
Saving after rename is explicit. Old unnamed templates remain readable with
Zapisana osoba as a UI fallback and can be named without recapture; old three-sample
references still require eight samples before recognition. Older binaries that
deny unknown template fields may reject a newly named file.

Template/pending name strings are zeroized on drop; GUI shared strings and
displayed labels are not guaranteed zeroized. Names are user-entered display
labels, not identity evidence, login accounts or IPC principals. The GUI still
holds/stores one person reference, not a gallery. No thresholds, image handling,
worker isolation, storage encryption or authentication-readiness gates changed.


### Scanner-first GUI — 2026-10-11

The Osoba workflow now follows scan → label → rescan/recognize. A user click
starts/resumes a bounded eight-probe enrollment; a cancellable 1.5-second pause
between probes presents the next pose instruction. A quality/worker failure or
Stop ends the run retaining accepted probes, without replacing the old reference.
Completion produces an unnamed template and prompts for a validated label; GUI
recognition is gated until the label exists. The same explicit encrypted save
persists name and template. Older unnamed references also receive the label prompt.

Optional FrameObserver delivers camera frames only in the unprivileged parent
to the GUI during acquisition. Existing APIs omit the observer. GUI RGB conversion
uses a zeroizing temporary; one pending image update bounds the event queue.
A per-job alive flag/cancellation check rejects late updates. Completion/cancel
clears the displayed image and availability flag. Slint image buffers are not
guaranteed zeroized; this is in-memory display, not persistent image storage.
No raw image/embedding is added to metadata. Existing bounded acquisitions still
join the camera, restore the scoped IR lease and reap the worker on each attempt.

The oval is a positioning guide, not detector coordinates. The animated line
indicates an active operation, including model initialization and pose pauses;
it is not progress evidence, liveness, or a continuous live video stream. Camera
frames update during acquisition, with at most 24 attempts per eight-sample scan
and nine per three-probe recognition. Actual accepted count drives progress.
UI rejects identity naming on errors, ambiguity and nonmatches as before.
One person reference, original threshold and all disabled authentication gates
remain unchanged. No scan starts automatically at application startup.


## Strict owner recognition and measured motion — 2026-10-11

This supersedes the earlier 0.363 / three-probe GUI identity policy. The
three-probe diagnostic comparison still returns mathematical scores only.
GUI recognition/trials now use `recognize_person`, a separate fail-closed path:
minimum/default threshold 0.80, centroid AND median of all stored exemplar
embeddings, plus nearest six-ratio landmark shape (maximum absolute component
change 0.30 and RMS <=0.15). These are provisional gates, not measured FAR/FRR.
New encrypted templates retain every exemplar and optional normalized geometry.
Old v1 templates still parse but cannot enter strict recognition without re-enrollment;
geometry and exemplar counts, dimensions/model identity and finite bounds are
validated when loading. All new private geometry is excluded from metadata/debug.

Five steps: front, random left/right, front, opposite turn, front. Parent reads OS
entropy before opening the camera. Front establishes the owner baseline from two
consecutive quality-accepted, identity-matching observations with normalized nose
offset difference <=0.05; no universal absolute zero offset is assumed. A turn
requires offset relative to this baseline in [0.06,0.45] with the requested
sign; returning front requires deviation <=0.05. Each step allows six valid pose
attempts; per-attempt quality retry stays bounded at three. All quality-accepted attempts
check identity; a score below 0.80 rejects immediately, even if pose is not reached.
Any lower score than the user-selected threshold also blocks a final match.
Each frame digest must be unique, camera/IR must stay bound, template revision must
stay fixed, cancellation and acquisition errors abort. No score history or images
are persisted. Only a completed five-step result can be positively classified.

This is **experimental active motion checking, not validated PAD**. YuNet provides
five points only: no eyelid landmarks for blinks, no facial contour/dense shape and
no measured depth. Prints/screens moved projectively, replayed video, masks and
video injection can still defeat motion heuristics. No RGB-trained PAD model was
silently applied to IR data. PAD/system authentication readiness remains false.
Real owner/impostor and spoof trials are still required on the actual camera;
software tests do not establish measured false-accept rates or spoof resistance.


## Usability fix: measured turns and IR settling — 2026-10-11

Following user-reported turn/exposure failures, required normalized nose motion is
0.06 instead of 0.10. The opposite turn, return-to-center, identity floor 0.80,
exemplar/shape gates, distinct frames and cancellation remain mandatory. UI now
shows measured motion progress (not identity confidence/PAD score) and a wrong-
direction/overturn hint. The hint stays visible while retrying the same pose.
Transient face-ROI exposure/quality failure or loss of face is retried within the
same six-attempt pose bound; worker/camera/protocol failures remain fatal. Multiple
faces, duplicate frames and low identity evidence still abort. Six failed attempts
never produce recognition; final error includes the actionable last quality hint.
Dark mean, bright mean and clipped pixels now have distinct rejection messages;
brightness range 0.12–0.88 and maximum clipped fraction 0.12 remain unchanged.
Camera selection now skips the first 18 IR / 8 RGB frames and prefers three
consecutive acceptable stable global exposures (mean change <=0.025 and contrast
change <=0.02), within existing 90/30 frame limits. Separate ROI quality gates
still apply after inference; no camera control/emitter payload changes.


## Initial baseline calibration and truthful feedback — 2026-10-11

The initial pose no longer requires absolute five-point nose offset <=0.12. That
uncalibrated frontal assumption can block an owner with consistent IR landmark
bias or natural asymmetry. Two distinct, consecutive quality-accepted samples
must match the saved owner and have nose offsets within 0.05. Their average is
the per-attempt baseline. A rejected-quality/missing-face observation resets the
pending baseline. Failure to get a stable pair within six attempts aborts; both
opposite measured turns and both returns remain mandatory, and exact duplicate
frames still fail. All usable identity observations, including discarded baseline/
wrong-pose samples, contribute to the final minimum score so a higher user-selected
threshold cannot be bypassed by omitting a preliminary acquisition from the summary.

UI emits movement percent only during left/right turn measurement. Baseline and
return-to-center show their instructions; rejected quality shows a quality title
and actionable reason, rather than inventing a measured 0% movement. Existing
strict encrypted templates remain compatible; no re-enrollment is needed.


## Howdy-based continuous acquisition — 2026-10-11

This section supersedes the earlier descriptions of reopening the camera/worker
on each person-scan attempt, stable whole-frame exposure selection, and the old
face-ROI brightness/clipping limits. Legacy single-probe diagnostic actions still
use the single-probe API. Source comparison, evidence and limitations are in
[Howdy analysis](howdy-analysis.md).

Person scanning, five-pose recognition and three-probe comparison now initialize
one restricted native worker before opening one camera session. Preview continues
throughout pose instructions, while an in-memory latest-frame watch buffer is
replaced continuously. Analysis awaits a new frame after each prompt pause; no
old queued pose frame or raw image history is used. IR gets 18 warm-up frames,
RGB eight, once per operation. On the first streamed frame the existing whitelisted
IR mode is reapplied and verified. No arbitrary UVC control or exposure value is
introduced. Cleanup joins capture, releases the latest private buffer, verifies
IR restoration and exits/reaps the worker before displaying the result.

Each sample searches for at most four seconds and four model inferences. The GUI
camera operation is limited to 50 seconds, the camera to 55 seconds; the worker
has a 60-second total wall bound, 192-frame budget, 20-second CPU limit, and the
existing eight-second per-transaction limit. Cancellation, changed camera/model,
duplicate analyzed frames, multiple faces, malformed replies and worker failures
remain failures. Per-frame face/geometry state is reset even on empty frames.

Whole-frame signal checks reject near-uniform/black/white frames, rather than
requiring the whole IR background to be bright. Histogram buckets use dark <32
and white >=248 of 255; either fraction >=99.5% is rejected, together with missing
contrast/edges. The detected face retains confidence >=0.90, minimum size 96 px,
coverage/border checks. Face mean 0.08–0.92, contrast >=0.03, edge energy >=0.005
and edge/contrast >=0.07 replace the old exposure-sensitive absolute sharpness
cutoff. Overexposed face pixels (>0.98) are limited to 12%, total black/white
clipping to 60%; dark hair/brows are no longer classified as bright glare.
These are acquisition heuristics, not calibrated biometric accuracy guarantees.

If the raw monochrome detector finds no face, CLAHE (clip limit 2, 8×8 tiles) is
applied for detection only. Alignment and SFace embeddings still use the original
pixels, preserving model binding and stored-reference compatibility. The strict
identity floor 0.80, stored-exemplar and geometry checks, two-frame owner baseline
and all five movement stages are unchanged. UI separately reports dark/uniform
camera input, no detected face, glare, face detail and model failures. No evaluated
PAD model or system authentication is enabled.


## Slower instructions and patient quality retries — 2026-10-11

New position instructions remain for three seconds before acquisition, including
the first enrollment position. Retries of the same position pause 0.8 seconds;
they do not restart the full instruction delay. This supersedes the old one-/
two-second recognition and 1.5-second enrollment pauses.

Recognition retries a pose up to 16 times within 15 seconds, instead of six
rapid attempts. Enrollment retries quality or no face up to eight times within
15 seconds per position, without discarding accepted samples. A typed transient
capture error represents dark/uniform frames and may be retried. Other camera,
model, protocol, duplicate-frame, multiple-face, cancellation and identity failures
remain fatal; arbitrary errors are never converted into recoverable quality.

Each live search runs at most four inferences, reducing model-budget consumption
while waiting for a pose. The 50-second operation / 55-second camera / 60-second
worker, 192-frame and CPU bounds remain unchanged. A stage limit and global limit
both apply; retries cannot grant a match when movement or identity is missing.
The owner similarity floor and geometry/exemplar gates remain unchanged.


## First-stage baseline across brief IR gaps — 2026-10-11

The first valid owner frame previously disappeared on every dark, no-face or
poor-quality capture. Alternating IR usability could consequently keep resetting
stage one despite a stable owner. The first-stage baseline now accepts two
quality-accepted, distinct, owner-validated frames within five seconds of each
other, with the same maximum normalized yaw difference 0.05. Unusable frames do
not count and may bridge a short gap. An expired or unstable candidate is replaced
with the newest owner observation, requiring another valid observation.

This supersedes the earlier requirement for consecutive good frames and resetting
the pending pair on any quality rejection. Wrong identity, multiple faces,
duplicates, changed reference and cancellation still abort. All measured usable
identity scores still contribute to the minimum; remaining turns/returns are
unchanged. New tests cover quality/darkness/no-face gaps, expiration, unstable
pairs and a different person trying to provide the second frame. Quality hints
now distinguish a small face, out-of-frame face and missing detail instead of
collapsing these into a generic unreadable-frame instruction. No real owner scan
was performed to establish that this was the user's exact blocking cause.


## Bounded transitional turn retries — 2026-10-11

A user reported immediate refusal on a slight turn while camera/IR worked.
The previous policy treated a single low strict similarity (including a geometry
mismatch mapped to -1) on a moving view as an immediate final mismatch. New turn
handling changes this decision policy; it does not lower the 0.80 identity floor
or change the six-ratio, centroid, median-exemplar, capture, model or storage gates.

On each of the two turn stages only, at most two below-floor/shape-mismatch frames
may be discarded and retried. They supply no identity/pose evidence and are not
included in the accepted identity minimum. A third such frame in the same stage
returns NotRecognized, even if good frames occurred between failures. The budget
is not reset by a good frame. Front/baseline and both returns still reject a low
identity result immediately. All owner-validated observations, including pending
pairs and wrong poses, still contribute to the final minimum.

Each turn now needs two distinct, quality-accepted, strictly matching owner frames
in the requested direction, yaw agreement <=0.05, within five seconds. Quality,
no-face, transient-capture, wrong-pose or weak-identity observations reset the turn
pair. One matching turn frame cannot complete a stage. Five completed stages are
still required. Summary metadata records `transition_identity_retries`; no pixels
or geometry are added to public reports. The existing 15-second/16-attempt stage
and global camera/worker budgets still apply.

Enrollment similarly retries at most two `InconsistentSample` errors per position
inside its existing quality/time budget. Rejected descriptors are never appended;
all eight stored samples still pass the unchanged 0.80 pairwise consistency gate.
Other errors, duplicates, multiple faces and cancellation stay fatal. The observed
collector supplies a localized live hint to reduce the turn and hold it still.
This section supersedes earlier claims that every weak identity observation on a
turn immediately aborts and contributes to the final minimum. This is a temporal
acquisition policy, not an evaluated improvement in biometric accuracy or PAD.


## Accuracy refactor / template v2 — 2026-10-12

Current component audit, concrete IR rejection evidence, matching/enrollment
policy, licensing/runtime decision and performance results are documented in
[accuracy improvements](accuracy-improvements.md). This supersedes the old
96-pixel minimum, all-prior-pose consistency for new native enrollment and the
all-exemplar median for new templates. V1 template scoring remains unchanged.
V2 uses three frontal anchors and measured-pose selection of three exemplars,
with centroid/median/shape checks and the existing owner floor. New templates
validate centroid integrity and model compatibility; re-enroll to obtain V2.

Worker readiness is LHRDY003. The private reply adds native cpu_time_us and
quality metadata inter_eye_pixels; rebuilding CLI/worker/GUI together is required.
Successful replies require validated quality and geometry. ROI quality precedes
alignment/embedding, and reports contain explicit quality/identity failure causes,
last-frame metadata, selected camera format, latency mean/P95/P99 and CPU/RSS.
GUI recognition displays the minimum used for its decision, rather than presenting
a high median that could conceal the rejected observation.

Manifest V2 in face-evaluate evaluates claimed-user template/sequence scoring,
with disjoint calibration and held-out identities, separate sessions, failed
acquisitions included in end-to-end FRR and optional face-count annotations.
It is not full GUI motion-protocol/PAD qualification. See
[verification-manifest.example.json](verification-manifest.example.json).
The existing V1 pairwise protocol remains available. Both use one bounded loaded
worker. face-benchmark --stream measures the same loaded-worker path; the original
cold benchmark remains available. No calibration is installed into PAM/GUI.


## Normal recognition without mandatory turns — 2026-10-12

The default Osoba → Skanuj i rozpoznaj and both labelled verification trials now
use verify_person_observed: one reference, three fresh distinct quality-accepted
frames, minimum owner similarity across all three, unchanged centroid/exemplar/
shape gates and floor0.80. No yaw calibration, randomized turn, nose displacement
or five-stage challenge is required. First acquisition pauses1s, subsequent
acquisitions0.3s. Quality/no-face/transient darkness may retry within16 captures
and15s; identity mismatch, multiple faces, duplicate frames, reference changes,
model/camera error or cancellation abort. A result arriving after the deadline
cannot complete verification. Camera/IR/worker cleanup still precedes final UI.

ComparisonSummary records verification_protocol: passive_front (exactly3 frames),
measured_motion (exactly5 completed stages and motion passed), or comparison_only
(the legacy score diagnostic). Classifier validates the explicit protocol; it does
not forge motion_challenge_passed for normal recognition. PAD/authentication flags
remainfalse. Final progress rechecks cancellation/reference and queued UI progress
cannot overwrite a finished result. A partial1/2 frame set cannot be recognized.

The existing measured-motion implementation remains available through a separate
Settings → Diagnostyka kamery i modeli → Test ruchu głowy (diagnostyka) button.
This supersedes earlier documentation requiring turns for ordinary recognition.
Enrollment and templates did not change; no additional re-enrollment is required
for this workflow change. Previously stored complete V1/V2 references still work.

The interaction resembles a look-at-camera scan; Windows Hello's hardware/PAD and
protected authentication are separate requirements. See Microsoft's
[Windows Hello face authentication overview](https://learn.microsoft.com/windows-hardware/design/device-experiences/windows-hello-face-authentication).
This application does not claim Windows Hello security equivalence.
