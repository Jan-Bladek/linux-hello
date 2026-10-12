# Recognition accuracy and reliability review — 2026-10-12

This is a focused refactor of existing Rust/OpenCV diagnostics, enrollment and
verification. It does not replace the GUI, camera transport, encryption, daemon,
systemd units, fingerprint integration or authentication interfaces.

## Analysis and focused implementation plan

1. Audit camera conversion, actual model contracts, matching and stored references.
2. Separate acquisition rejection from embedding/shape/identity rejection.
3. Preserve pinned inference and introduce versioned pose-aware template matching.
4. Capture frontal anchors before collecting reasonable pose variation.
5. Measure the actual claimed-user multi-frame scoring protocol, including failures.
6. Test native alignment, grayscale processing, worker isolation, storage migration
   and GUI; measure functional latency/CPU/RSS without inventing population accuracy.

These implementation steps are completed. Calibration on representative consented
NIR data, owner recognition across sessions and PAD qualification remain external
validation steps. No such dataset was supplied. The default 0.80 diagnostic floor
remains explicitly provisional rather than a calibrated authentication threshold.

## Existing architecture and actual failure causes

| Component | Audited implementation |
| --- | --- |
| Capture | V4L2/MMAP, checked GREY/RGB24/YUYV conversion; primary IR `/dev/video2` GREY 640×360, RGB `/dev/video0` YUYV 640×480; explicit bounded capture only |
| Temporal capture | One camera/emitter/restricted worker per operation, newest-frame buffer, distinct analyzed frame digests, cancellation and verified cleanup |
| Detector | Pinned ONNX YuNet 2023mar on OpenCV CPU; five landmarks; detector-only CLAHE for monochrome |
| Recognition | Pinned ONNX SFace 2021dec, OpenCV five-point similarity alignment to 112×112, raw BGR bytes converted to RGB NCHW by `feature()`, 128-dimensional L2-normalized embeddings |
| Comparison | Cosine dot product, model/dimension binding, one explicitly selected reference, no search across accounts |
| Storage | Explicit XChaCha20-Poly1305 reference encryption, Argon2id-derived key, UID binding, private directories/files and bounded atomic access; model IDs and exemplars encrypted |
| Threshold | Experimental default/floor 0.80, adjustable upward in GUI; no representative sensor calibration was present |
| Authentication | `hello-pam` is a readiness stub (`IMPLEMENTED=false`), not working PAM authentication. Existing privileged-authentication gates remain disabled; production PAM was not changed |

The input contract was checked against [OpenCV 4.13's actual implementation](https://github.com/opencv/opencv/blob/4.13.0/modules/objdetect/src/face_recognize.cpp).
Applying ArcFace-style `(pixel-127.5)/127.5` normalization here would violate SFace's
selected wrapper contract. Alignment and descriptor pixels therefore remain
unchanged; contrast enhancement is only for detection.

Three problems are supported by code/measurement evidence:

* Legacy enrollment demanded similarity >=0.80 to **every** prior pose. Two views
  can each match frontal anchors but not each other, especially opposite turns.
* Legacy verification used a median over **all** poses; unrelated opposite views
  can dominate a turned probe. Synthetic arithmetic regressions reproduce this
  rejection without changing the model or threshold. This is a scoring defect,
  not proof of measured population FRR.
* Actual IR diagnostics detected one face at confidence 0.9027, width 91 and
  height 110, coverage 4.34%, mean 0.303, contrast 0.062 and edge energy 0.0102.
  It was rejected by the old 96-pixel width cutoff, not illumination or absence
  of a detected face. Later acquisition without a visible detected face remained
  a no-face result; it was not presented as owner recognition.

No evidence justified replacing the trained embedding model before fixing these
gates. Remaining model error, optimal threshold and NIR generalization cannot be
identified quantitatively without representative genuine/impostor data.

## Implemented changes

New native enrollments produce **template v2**. First collect three frontal
anchors (normal, slightly closer, slightly farther), then left/right turns,
chin up/down and front. Anchor yaw is relative to the first observation, so a
natural landmark offset is not mistaken for a turn. A premature turn is retried
with an explicit instruction. The first three embeddings remain mutually
consistent. Later samples must match the anchor centroid and at least two of
the three anchors at the unchanged consistency floor. Rejected samples never
mutate the reference. Legacy diagnostic samples without full geometry stay v1.

V2 selects the three nearest stored **measured yaw** values before calculating
identity scores. It takes the median of those three similarities and the full
template centroid similarity; both must pass. It also requires a compatible
stored shape among the selected references. Selection never chooses the highest
similarity. One favorable exemplar cannot override the other two or the centroid.
All existing temporal owner/turn/return checks, distinct frames, cancellation and
bounded transition retry policy remain in force.

V1 encrypted templates retain their original scoring. V2 loading requires full
model-compatible exemplars/geometry and validates the centroid against the
exemplars. Unknown versions and corrupt or mixed-model templates fail closed.
The model/preprocessing did not change, but **re-enrollment is required to benefit
from v2 anchors and matching**. There is no automatic conversion, deletion or
decryption. Old v1 references continue working with this application; older
application binaries cannot load v2, so update CLI/GUI/worker together.

Quality assessment now uses a minimum 80×80 face, the existing coverage/detail/
exposure gates, and at least 24 source pixels between eyes. SFace's canonical eye
distance is approximately 35.24 pixels; the eye condition limits enlargement to
about 1.47× instead of assuming face-box width directly determines usable detail.
These remain provisional acquisition heuristics, not biometric thresholds. Blurred,
clipped, tiny-eye, uncertain and multi-face frames remain rejected. Tracked native
inference checks ROI quality and valid geometry before alignment/descriptor work;
successful worker replies must contain both valid quality and geometry.

YuNet now retains >=0.75 presence candidates to avoid silently hiding a weaker
second face. **A descriptor still requires exactly one face and confidence >=0.90.**
Monochrome CLAHE also retries a single low-confidence detection, preserving any
multi-face outcome. Native boxes intersecting the image are clipped to the image;
border quality still rejects cropped references. Original descriptor pixels are
never equalized or resized twice.

Development reports include device, selected format/dimensions, quality rejection,
detection confidence, embedding status, pinned recognizer/preprocessing, centroid
and exemplar score components, template version, selected count, failure reason,
active GUI threshold, latency and native CPU/RSS. Latest-frame reports contain
metadata only, with no recursive report history. Embeddings/landmarks/images are
not logged. Worker readiness is `LHRDY003`; mismatched binaries fail before capture.

## Why SCRFD + ArcFace + ONNX Runtime was not enabled

The stack is a reasonable candidate for a separately qualified model adapter.
It was evaluated against primary sources:

* [InsightFace model zoo](https://github.com/deepinsight/insightface/blob/master/python-package/docs/model_zoo.md)
  restricts its supplied pretrained weights to non-commercial research, independent
  of the code license. No selected licensed, qualified NIR checkpoint was provided.
* [SCRFD source](https://github.com/deepinsight/insightface/blob/master/python-package/insightface/model_zoo/scrfd.py)
  requires its own resizing, scale restoration, output decoding and input contract.
* [ArcFace ONNX source](https://github.com/deepinsight/insightface/blob/master/python-package/insightface/model_zoo/arcface_onnx.py)
  selects normalization based on the graph, and aligns to its expected input.
  Its embeddings are not compatible with stored SFace descriptors.
* [ONNX Runtime language APIs](https://onnxruntime.ai/docs/api/)
  list the Rust API as a community integration. A new runtime also needs the
  existing memory, threading, syscall, artifact and private-pipe isolation tests.

An unqualified RGB model swap would not establish better NIR accuracy. Pinned
YuNet/SFace are retained for local development functionality, **not certified NIR
authentication**. No new runtime dependency or model download was introduced.

## Measurement protocol and available evidence

`face-evaluate` accepts manifest v1 for legacy pairwise cosine evaluation and
**v2 for actual template/sequence 1:1 scoring**. The v2 example is
[verification-manifest.example.json](verification-manifest.example.json).
Each enrolled identity needs eight ordered frames in an enrollment session.
Each attempt explicitly names its claimed identity and contains 2–12 frames of
one actual subject in a different session. Calibration/evaluation identities are
disjoint, and both splits need genuine and impostor attempts. Duplicate bytes,
changed files, model failures, invalid claims, mixed attempt subjects and session
leakage fail before scoring. No image capture or enrollment persistence occurs.

```sh
target/release/hello face-dataset-check --manifest /path/to/consented/manifest-v2.json
target/release/hello face-evaluate --models work/inference/models \
  --manifest /path/to/consented/manifest-v2.json --target-far 0.01
```

Calibration searches only calibration attempts and preserves the minimum 0.80
diagnostic policy. Multi-frame attempt score is the minimum of all supplied
suitable frames. Missing/low-quality frames or an unenrollable reference cannot
produce success. Held-out reports include end-to-end FAR/FRR (including failed acquisitions),
conditional FAR/FRR on scorable attempts, failure-to-acquire, denominators and
descriptive Wilson intervals. Calibration uses scorable impostor attempts, so
camera failures cannot manufacture a permissive threshold.
Optional `expected_faces` annotations provide **face-count agreement**, not
bounding-box detection AP. Thresholds are not installed into GUI/PAM automatically;
sensor/provenance strings are declarations, not trusted attestations. Without
adequate scorable attempts no threshold/evaluation is reported as available.
This evaluates the identity sequence, not the GUI’s full five-pose protocol or
its bounded transitional-frame retries; FAR/FRR must not be transferred to that
complete workflow without a separate protocol study. Repeated-subject attempts are correlated; the intervals do not establish population
confidence. No real FAR/FRR or detection-accuracy number has been measured here.

Functional performance was measured on this Fedora host using 20 repetitions of
the existing public 512×512 RGB fixture, not a consented identity evaluation set:

| Mode | Native mean / P95 | Request mean / P95 | Startup | Native CPU for 20 frames | Peak RSS |
| --- | --- | --- | --- | --- | --- |
| Fresh worker each frame | 68.25 / 71 ms | 112.05 / 142 ms | mean 152.45 ms, 20 starts | 1,366,666 µs | 207,736 KiB |
| One loaded worker | 37.7 / 41 ms | 77.7 / 102 ms | 147 ms once | 754,751 µs | 207,568 KiB |

All 20 frames in each run produced embeddings. Request timing excludes startup;
CPU excludes model initialization and is measured around native pipeline work.
Peak RSS is process high-water usage, not the 1 GiB address-space limit. P99 is also
reported, but 20 samples are insufficient for a meaningful tail-latency study.
This demonstrates the warm-worker benefit, already used by GUI scanning, and does
not establish an accuracy improvement or a before/after user-verification latency.
The previous release's cold native median/P95 was 67/71 ms: no cold-model speedup
is claimed. Raw metadata logs are ignored under `work/inference/accuracy-20261012`.

Reproduce both current modes with `face-benchmark` and add `--stream` for one
loaded worker. No live camera or biometric reference is needed for that command.

## Validation and limits

145 feature-enabled workspace tests and 73 default workspace tests pass. Explicit
native tests verify canonical alignment, color order, grayscale/dim inputs,
128-dimensional output, blank/two-face rejection and restricted streaming worker
cleanup. Actual IR tests verified capture, metadata and emitter restoration; an
owner verification after re-enrollment was not performed. New synthetic tests
cover opposite-pose scoring, unknown/incompatible embeddings, insufficient/mixed
frame sequences, corrupt v2 templates, enrollment anchors, dataset claim/session
leakage and end-to-end acquisition failures. Existing camera, GUI rendering,
storage, timeout and cancellation tests remain passing. Clippy with warnings denied,
formatting and release CLI/GUI/worker builds pass.

Re-enroll in the new GUI to obtain v2, then test owner, other consenting people,
moderate poses and separate lighting/session conditions. Evaluate spoofing
separately. Five landmarks and randomized movement are not validated PAD or depth;
Windows Hello security equivalence and production authentication are not claimed.


## Follow-up: ordinary recognition without a motion challenge — 2026-10-12

Normal recognition now uses three suitable distinct matching frames on a short
look-at-camera scan. It does not require the five-pose test discussed above; that
experiment is retained as a separate diagnostic button. Reports explicitly label
passive_front, retain motion_challenge_passed=false and pad_validated=false, and
still require every accepted frame to satisfy all identity gates. Existing V1/V2
references remain compatible. This workflow change needs no model/template
migration or extra enrollment. See the current inference workflow for limits.
