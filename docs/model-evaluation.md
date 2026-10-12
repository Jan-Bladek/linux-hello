# YuNet/SFace review and local evaluation — 2026-10-11

## Current decision

Pinned YuNet/SFace are accepted for explicit local functional evaluation only.
No production biometric threshold, accuracy or redistribution approval is claimed.
Model bytes, input contract and artifact license evidence are recorded in
[inference diagnostics](inference.md). This review adds the following evidence.

| Item | Observed evidence | Outstanding gate |
|---|---|---|
| YuNet exact bytes | SHA-256/length pinned; OpenCV Zoo directory states MIT | Complete weight/training-data rights chain and notices |
| YuNet training source | [Pinned training repository](https://github.com/ShiqiYu/libfacedetection.train/tree/a61a428929148171b488f024b5d6774f93cdbc13) describes WIDER Face with relabeled annotations | Exact checkpoint lineage and dataset/annotation terms |
| SFace exact bytes | SHA-256/length pinned; OpenCV Zoo directory states Apache-2.0 | Exact checkpoint-to-training-dataset mapping and downstream rights review |
| SFace architecture | [OpenCV Zoo README](https://github.com/opencv/opencv_zoo/blob/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_recognition_sface/README.md) identifies MobileFaceNet with SFace loss | No implication that paper benchmark results apply to this camera |
| Training provenance | [Original SFace repository](https://github.com/zhongyy/SFace) discusses CASIA-WebFace, VGGFace2 and MS-Celeb-1M experiments | It does not map the selected hash to one exact training dataset |
| Upstream clarification | [Issue 313](https://github.com/opencv/opencv_zoo/issues/313) is open; GitHub API checked on 2026-10-11, four comments, all request clarification | No authoritative dataset/weight-permission clarification was found there |
| Native format | Both selected artifacts use default-domain ONNX opset 11, no custom operator domains in header inventory | Inventory is not formal ONNX validation or malicious-model certification |
| Runtime compatibility | Actual CPU detection, alignment and 128-dimensional output pass inside the restricted worker | Accuracy, PAD, broader devices and verified IR remain unqualified |

The local license/readme/provenance copies and issue API evidence are under ignored
`work/inference`. The issue comments are questions from users, not authority to
grant rights. No issue/email was sent on the user's behalf. No weights or training
data were added to source/RPM packages. Dataset rights cannot be inferred from a
code SPDX identifier; this is an unresolved project adoption gate, not a legal
opinion on a specific deployment.

## Exact artifact inventory

Run without any model runtime or downloads:

```sh
python3 -B tools/inspect-face-models.py --models work/inference/models
```

This stdlib tool verifies pins, then reads bounded protobuf fields according to
[ONNX's schema](https://github.com/onnx/onnx/blob/v1.19.1/onnx/onnx.proto3).
It reports opsets, runtime inputs, outputs, initializer counts and operator domains.
It skips initializer payloads rather than interpreting tensors or executing nodes.
It is a header inventory, not a complete protobuf/ONNX validator.

YuNet has float32 NCHW `input` [1,3,640,640] in the serialized model, with cls/obj/
bbox/kps outputs at three strides. OpenCV 4.x adapts this detector to the actual
image dimensions as described by its [upstream README](https://github.com/opencv/opencv_zoo/blob/47534e27c9851bb1128ccc0102f1145e27f23f98/models/face_detection_yunet/README.md).
SFace runtime input is float32 NCHW `data` [1,3,112,112]; `fc1` is [1,128].
Initializer-backed legacy graph inputs are counted separately from runtime inputs.
This matches the tested adapter contract; no arbitrary models are accepted.

## Reproduce functional performance

Build both CLI and worker; they must remain next to one another:

```sh
cargo build --locked --release -p hello-cli --features face-opencv
target/release/hello face-benchmark \
  --models work/inference/models --rgb work/inference/upstream-lena.rgb \
  --width 512 --height 512 --iterations 5
```

The public fixture is prepared explicitly as described in `inference.md`. Each
iteration loads a new restricted worker and sends one raw RGB frame through private
pipes. The JSON reports startup, native inference and supervised-request timings,
minimum/median/nearest-rank P95/maximum, accepted iterations and peak worker RSS.
The startup phase includes spawning until the ready handshake; request latency
includes pipe transfer, output validation and worker exit. RSS is Linux `ru_maxrss`
in KiB; the hard address-space limit is a different quantity. The default benchmark
is only five repeated inputs, and is not a representative tail-latency study.

The recorded debug run accepted all five public-image iterations: native inference
median 89 ms, P95 94 ms, startup median 1094 ms, maximum RSS 208184 KiB.
The release run also accepted all five iterations: native median 64 ms, P95 71 ms,
startup median 147 ms, maximum RSS 207960 KiB (about 203 MiB). Both runs
have **no FAR/FRR output**. These timings cover one host and one public input,
not genuine/impostor performance or PAD.

## Genuine/impostor evaluation

`hello face-evaluate` accepts only an explicit local manifest. It never downloads,
captures, enrolls, or persists vectors. No suitable consented identity/session
collection has been supplied in this workspace, so population FAR/FRR were not
measured and no numeric authentication threshold was selected.

Copy [the manifest example](evaluation-manifest.example.json) next to your locally
authorized dataset and replace every placeholder. Provide these records:

- Dataset ID, consent/usage-rights references, actual sensor mode and conditions.
- 8–128 uniquely named samples with pseudonymous identity, acquisition session,
  calibration/evaluation split, relative raw RGB path and dimensions.
- At least two identities per split, disjoint identities across splits, and at
  least two acquisition sessions for every identity. Eight is a software minimum, not a
  sufficient security evaluation population.
- Raw RGB files (16–1920 by 16–1080), exactly width×height×3 bytes. Final symlinks,
  nonregular/oversized files, parent/absolute paths and duplicate image bytes are
  rejected. Parent-directory symlinks are not a trusted data-custody boundary;
  datasets remain explicitly trusted local inputs.

```sh
target/release/hello face-dataset-check \
  --manifest /absolute/path/to/dataset/manifest.json
target/release/hello face-evaluate --models work/inference/models \
  --manifest /absolute/path/to/dataset/manifest.json --target-far 0.01
```

`face-dataset-check` checks the complete manifest and every image without loading
model artifacts, starting capture or calculating embeddings. It reports aggregate
split sample/identity/session counts, the manifest SHA-256 and a dataset SHA-256
that binds the manifest bytes plus ordered image digests. Individual image digests,
paths and identity/session labels are omitted. The check retains only one bounded
image buffer at a time and discards it after hashing. Cancellation aborts the check.
It checks that provenance references are present, not that consent, licenses,
session labels or claimed sensor mode are factually correct; accordingly
`provenance_verified` remains false. Success supplies no recognition accuracy.

`face-evaluate` now performs this same complete check before the first worker
starts. Missing/short files or duplicates even at the end of a dataset therefore
abort before inference. Each image is read again immediately before processing
and its digest must match the checked bytes; replacements abort the evaluation.
The report includes the aggregate dataset digest as well as the manifest digest.
This is reproducibility and change detection, not a trusted acquisition guarantee
or a filesystem snapshot. SHA-256 identifiers can correlate repeated datasets;
retain aggregate reports as intended diagnostics.

The target above is an example empirical calibration policy, not a project security
claim. Calibration chooses the smallest allowed cosine threshold meeting that
empirical impostor target, using only calibration identities. It reports calibration
and held-out pairwise false acceptance/rejection counts, denominators, acquisition
failures and descriptive Wilson intervals. A score equal to the threshold is
accepted. Same-identity/same-session pairs are excluded. Missing acquisitions stay
in attempted-sample/pair counts; match rates are conditional on usable acquisitions.
Insufficient genuine/impostor pairs or an impossible target cannot produce a
successful evaluation. Worker crashes/timeouts abort the run; they are not silent
sample omissions. Unknown manifest fields fail before any model/image processing.

The report contains aggregates and the manifest digest, not identity/session names,
image names, landmark coordinates or vectors. Pseudonymous dataset ID is reported;
choose one that does not contain personal information. The threshold is diagnostic,
never installed into enrollment, daemon or PAM configuration.

**Statistical limit:** all-pairs scores share subjects/images and are correlated.
The included binomial Wilson bounds are descriptive under an independence
assumption, not validated population confidence intervals. Zero observed errors
still have a nonzero upper bound. Real qualification needs a prespecified protocol,
identity/session cluster bootstrap or independent attempt design, larger held-out
populations, lighting/pose/glasses/skin-tone coverage, time separation and independent
PAD/acquisition evaluation. Pairwise feature comparison is not a test of the future
multi-sample enrollment workflow. RGB results do not establish NIR applicability.
