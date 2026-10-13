# Biometric primitives and release gates

This development build contains real local matching, multi-sample template construction,
image quality statistics, authenticated encryption, and Linux secure file operations.
An optional CPU YuNet/SFace adapter now performs detection, alignment and ONNX
inference for the GUI and experimental verifier; see [its contract and reproduction steps](inference.md).
It does **not** contain security-qualified recognition or validated presentation
attack detection. Administrator-authorized protected import and experimental PAM
verification are implemented separately; see [authentication flow](authentication-flow.md).
Model weights are fetched only by an explicit utility command, never during application execution or build.

`FaceEngine` separates detection, alignment, and embedding extraction. The default
engine is `UnconfiguredEngine`, which returns `Unsupported` for every operation.
The optional OpenCV adapter supplies functional diagnostic inference in a restricted
worker. Synthetic embeddings exercise unit tests; explicit public fixtures and local
camera probes supply functional checks, without authentication credentials.
The public mathematical matcher cannot issue an authentication credential.

`extract_embedding` now coordinates these steps for a configured diagnostic adapter.
It validates the source frame, model specification and image quality before calling
the detector; requires exactly one face; checks finite in-frame bounds/landmarks and
a caller-supplied confidence threshold; validates the aligned tensor's exact shape,
element count and finite values before inference; and rejects embeddings whose model
digest or dimension differs from the declared specification. Every failed boundary
stops the subsequent operation. `UnconfiguredEngine` remains unavailable through this
entry point.

Adapters implement `model_spec()` with the recognizer artifact digest, fixed input
shape and output dimension; `model_digest()` derives its answer from that specification.
Inputs allow 16–1024 pixels per side and one or three channels; embeddings allow
1–2048 elements. Detection bounds use source-pixel `[x, y, width, height]` and must
fit inside the frame. Landmark ordering, normalization and channel order still require
the adapter's documented model contract. This coordinator implements no alignment
transform, ONNX inference, model-file verification, trusted acquisition or PAD.

## Model research, checked 2026-10-10

| Candidate | Upstream licensing information | Decision for this build |
|---|---|---|
| OpenCV Zoo YuNet detector | Directory states MIT; current ONNX export supports dynamic dimensions | Selected for local functionality; qualification incomplete; not bundled |
| OpenCV Zoo SFace/MobileFaceNet recognizer | Directory states Apache-2.0; ONNX and five-landmark alignment documented | Selected for local GUI and experimental PAM functionality; NIR/PAD/provenance qualification incomplete |
| fal AuraFace-v1 recognizer | Model card labels Apache-2.0; ResNet100; acknowledges demographic limitations | Alternative evaluation candidate; device memory/latency unmeasured |
| InsightFace provided pretrained packs | Code and weights have different terms; provided weights are restricted to noncommercial research | Do not redistribute as general-purpose Linux authentication weights |

Sources: [YuNet upstream README](https://raw.githubusercontent.com/opencv/opencv_zoo/main/models/face_detection_yunet/README.md),
[SFace upstream README](https://raw.githubusercontent.com/opencv/opencv_zoo/main/models/face_recognition_sface/README.md),
[SFace license](https://raw.githubusercontent.com/opencv/opencv_zoo/main/models/face_recognition_sface/LICENSE),
[AuraFace publisher model card](https://huggingface.co/fal/AuraFace-v1),
[InsightFace model licensing](https://github.com/deepinsight/insightface/blob/master/python-package/docs/model_zoo.md).

Upstream benchmark results are not a measured false acceptance rate for this laptop,
IR images, or Linux authentication. Small public-fixture latency/RSS measurements
are recorded in [model evaluation](model-evaluation.md). We have not measured biometric
accuracy, demographic performance or physical attack resistance. Published RGB results do not
establish suitability for the Yoga Slim 7 infrared stream.

The latest documented Rust `ort` API was `2.0.0-rc.13` at inspection. It is a release
candidate, so this stable-dependencies build does not silently adopt it. ONNX Runtime's
C API is another possible adapter boundary but would require reviewed FFI and matching
packaged runtime versions. See [ort API](https://docs.rs/ort/2.0.0-rc.13/ort/) and
[official ONNX Runtime C API guide](https://onnxruntime.ai/docs/get-started/with-c.html).

Before implementing an adapter, pin every detector/recognizer artifact by SHA-256,
record its license, dataset provenance, ONNX opset, input shape/layout/color range,
alignment transform, output dimension, and supported runtime. Reject missing,
modified, incompatible, unexpectedly shaped, or non-finite outputs. Disable network
model discovery and custom operator loading. A model supplied by an unprivileged
user must never be loaded into the privileged verifier.

## Implemented invariants

* Frames have bounded dimensions (16–4096), checked stride arithmetic, explicit Gray8
  or RGB8 format, and an exact byte count. Statistics use a bounded sampling grid.
* Quality measures exposure, contrast, and local edge magnitude. None establishes
  that an image contains a face, that its source is a physical camera, or liveness.
* Embeddings contain 1–2048 finite elements, are normalized using an f64 norm, and
  carry the digest of the exact model. Empty/zero-norm/non-finite vectors are rejected.
  Matching requires identical model identity and dimension.
* Enrollment takes 3–12 distinct sample digests, requires exactly one detected face,
  applies quality checks, checks pairwise consistency, and creates a normalized
  centroid. Capture adapters must calculate sample digests and metrics themselves.
* Policy thresholds reject NaN, infinities, and values outside [0,1]. The current
  strict verification floor 0.80 is provisional; no calibrated FAR target is claimed.
* Template JSON has a 64 KiB limit, strict fields/version/sample count validation,
  and a dimension-bounded custom embedding deserializer. Sensitive vectors and
  plaintext buffers are zeroized on drop, including deserialization failures.

The diagnostic state machine binds events to a per-attempt challenge, enforces order,
uses a monotonic deadline, and rejects cross-user contexts. Similarity cannot replace
PAD. Missing PAD, attack detection, weak/non-finite PAD confidence, cancellation,
reused events, and mismatched evidence all terminate the attempt. Context values must
come from verified transport credentials; these Rust types are not IPC payloads.
`SystemAuthentication` is unconditionally rejected, and `authentication_ready()` is
false even if a caller changes reporting flags. This diagnostic state machine cannot issue a passing system authentication result;
the separately opted-in PAM adapter authenticates through the root verifier and
does not use this readiness state as authorization.

## Encrypted template storage

`hello-storage` uses the established RustCrypto XChaCha20-Poly1305 implementation,
with a random 192-bit nonce for every write and a 256-bit external key. The binary
envelope authenticates the format version, numeric owner UID, nonce, declared length,
and ciphertext. Cross-owner loads fail. Altering any envelope byte, using another key,
truncating data, or exceeding the size bound fails without returning a template.

Linux `SecureStore` opens an existing absolute private directory by walking every
component using `openat(O_DIRECTORY|O_NOFOLLOW)`. It retains the directory descriptor
and uses only fixed or random basenames relative to it. The directory must belong to
the effective UID and forbid group/other access. Non-root processes may address only
their own UID namespace. Root callers must separately authorize the target UID before
calling this library; a file API cannot establish the user's consent.

Writes create a random 0600 file with `O_EXCL|O_NOFOLLOW`, write encrypted data, fsync
it, atomically rename it over the template entry, and fsync the directory. The temporary
entry is removed on ordinary failure. Reads use `O_NOFOLLOW|O_NONBLOCK`, then validate
the opened descriptor is an owned private regular file with exactly one hard link and
a bounded size. Symlink swaps cannot redirect these descriptor-relative operations.
Multiple writers use atomic replacement; caller coordination determines which
successful save is latest in the generic external-key API. The optional GUI
passphrase workflow holds an exclusive advisory directory lock across its
read/verify/save/delete operation. Filesystem failure after rename can report an error after the
new entry has become visible; callers must re-read status before retrying.

Deletion unlinks the current entry and fsyncs the directory. It cannot guarantee
erasure from SSD remapping, journal history, snapshots, or backups. Encryption does
not prevent rollback of an older authentic template under the same key. Revocation
across backups needs per-enrollment key rotation plus a trusted generation counter.

The generic API receives keys in memory. Optional GUI diagnostic persistence
derives a key from a separate user-entered reference passphrase using Argon2id;
the GUI stores no plaintext key file. The separate PAM profile uses a random
root-owned key file, not the GUI passphrase or a daemon/TPM key service. See
[inference diagnostics](inference.md#multi-sample-encrypted-diagnostic-reference)
for format, parameters and limitations. Diagnostic security-readiness flags remain false; protected administrator import
exists for experimental PAM, while independent storage/key qualification remains open. Windows
supports only portable cryptographic envelope tests, not filesystem persistence.
Zeroization reduces residual memory; it does not provide locked pages, swap exclusion,
or protection against a compromised process/root. Set service core dump limits and
evaluate mlock/protected workers as part of the later deployment review.

TPM 2.0 sealed keys could protect a wrapping key against some offline theft. A future
design must review PCR policy, measured boot, updates, recovery and key rotation,
and distinguish key release from trustworthy live camera capture. No hardware-backed
authentication or TPM storage is implemented or advertised.

## Required evaluation before a security-qualified release

1. Obtain consented genuine/impostor datasets covering the actual IR/RGB modes,
   lighting, pose, glasses, skin tones, time separation, and enrollment conditions.
   Separate identities and sessions between calibration and held-out testing.
2. Report false acceptance/rejection with confidence intervals and sample counts;
   evaluate threshold stability and failure-to-acquire/enroll separately. Do not
   choose a threshold using the same held-out attack set used to report results.
3. Measure cold/warm p50/p95/p99 inference latency, CPU usage, peak RSS, camera startup,
   cancellation, and total wall-clock timeout on the specified Lenovo hardware.
4. Independently evaluate printed photos, phone/video replay, masks and display
   variations, camera replacement, injected frames, and replayed PAD results. A single
   IR image and a blink prompt are insufficient evidence for release.
5. Review the trusted acquisition path, protected model/key provenance, enrollment
   authentication, independent PAD binding, IPC principal checks, and recovery paths.
   Use isolated PAM configurations/private mount namespaces and disposable Fedora
   VMs; real host activation requires explicit authorization and recovery.

The present tests exercise algorithms and failure handling using synthetic values;
they are not evidence of biometric accuracy or physical attack resistance.


## Accuracy refactor — 2026-10-12

New native diagnostic enrollment uses version2 templates and three frontal
anchors before pose variation; old full version1 references retain their scoring.
See [the current component audit and limits](accuracy-improvements.md). Protected
storage, account access rules and model binding remain in force. Optional PAM
integration was added after this accuracy refactor; default diagnostic readiness
flags remain unchanged. Face processing is local. Pose-conditioned matching, grayscale
conversion and ordinary movement checks do not establish PAD. No representative
NIR population error rates or production authentication readiness are claimed.
