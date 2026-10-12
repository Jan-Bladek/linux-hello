# Howdy source analysis and acquisition repair

Reviewed on 2026-10-11 against upstream commit
`d3ab99382f88f043d15f15c1450ab69433892a1c` (MIT). The source checkout is under
ignored `work/inference/howdy-upstream`; Howdy was not installed or connected to
PAM. This is an independent Rust implementation of acquisition patterns, not a
port of Howdy's authentication policy.

## What the source does

| Source | Behavior and implication |
| --- | --- |
| [compare.py](https://github.com/boltgolt/howdy/blob/d3ab99382f88f043d15f15c1450ab69433892a1c/howdy/src/compare.py) | Loads dlib detection, landmarks and 128-dimensional recognition once. Keeps one camera open and loops until match/timeout. Applies CLAHE to grayscale detection input; landmarks and descriptors use the original image. Compares stored descriptors by Euclidean distance. |
| [recorders/video_capture.py](https://github.com/boltgolt/howdy/blob/d3ab99382f88f043d15f15c1450ab69433892a1c/howdy/src/recorders/video_capture.py) | Persistent OpenCV capture, grayscale conversion, optional exposure/FPS and alternative recorder backends. The defaults do not blindly force an exposure value. |
| [cli/add.py](https://github.com/boltgolt/howdy/blob/d3ab99382f88f043d15f15c1450ab69433892a1c/howdy/src/cli/add.py) | Loops over frames, rejects darkness/no face/multiple faces, then records a labelled descriptor. Does not demand five poses for this acquisition. |
| [config.ini](https://github.com/boltgolt/howdy/blob/d3ab99382f88f043d15f15c1450ab69433892a1c/howdy/src/config.ini) | Default timeout 4 seconds, dark threshold 60%, certainty 3.5. Certainty is divided by ten for Euclidean distance: 0.35 is not a SFace cosine threshold. Rubberstamps are disabled by default. |
| [rubberstamps/nod.py](https://github.com/boltgolt/howdy/blob/d3ab99382f88f043d15f15c1450ab69433892a1c/howdy/src/rubberstamps/nod.py) | Tracks smoothed nose movement relative to eye separation and counts directions. A movement challenge is not an evaluated presentation-attack detector; configured failsafe behavior must not be copied into strict recognition. |

Howdy's grayscale histogram has eight bins. Fully black frames are skipped;
the first bin approximates pixels below 32/255. Its detection preprocessing and
distance threshold belong to different models than our pinned YuNet/SFace pair.
Copying its numeric certainty or whole-frame darkness cutoff would not establish
IR accuracy here. Its README explicitly acknowledges printed-photo/lookalike
limitations. See [upstream security notice](https://github.com/boltgolt/howdy/blob/d3ab99382f88f043d15f15c1450ab69433892a1c/README.md).

## Problems found in Linux Hello

The previous person workflow repeatedly reopened the camera and started/loaded
models for each sample. IR startup and alternating illumination were mixed with
whole-frame exposure stability checks. A dark background could reject a readable
face before detection. Face sharpness used an exposure-sensitive fixed cutoff;
black pixels in hair and eyebrows were counted together with white glare.
The interface collapsed several causes into an unreadable-shot message.

The available physical measurements support an acquisition problem, but are not
an identity-accuracy experiment. Earlier single-probe IR measurements were nearly
black (whole-frame mean approximately 0.00000029 before stream rearming and
0.000051 after it). RGB delivered an image (mean approximately 0.265), with no
face detected in that unattended measurement. Jan confirmed the shutter is open
and the emitter visibly on. This rules out assuming a closed shutter as the fix.

Two explicit tests of the new continuous IR session each observed 39 frames,
skipped six and seven dark frames respectively, and each performed ten model
inferences on signalled frames. All reported no face. Capture stopped, the original emitter state was verified as
restored and the worker exited cleanly. This establishes that useful signal
arrives in a persistent stream; it does not establish recognition of Jan or a
measured false-accept/reject rate. Only metadata was saved.

## Implemented changes

One camera, scoped emitter lease and restricted loaded worker now serve a whole
person operation. A single latest-frame buffer supplies live preview and fresh
post-prompt samples, without retaining an image history. Each frame is tested for
signal before inference; short dark intervals are skipped. Model face/landmark
state is cleared per frame, including after an empty image. Monochrome detection
falls back to CLAHE, while alignment and embeddings retain original pixels.

Quality now separates whole-camera signal from detected-face usability, black
clipping from overexposure, and relative detail from absolute edge strength.
The UI explains the observed cause. Exact limits and bounded worker/camera
lifecycle are documented in [inference.md](inference.md#howdy-based-continuous-acquisition--2026-10-11).

The owner similarity floor 0.80, stored-exemplar and shape gates, distinct frames,
randomized turns, returns and two-frame owner baseline remain mandatory. Existing
encrypted templates retain compatibility. No photos or embeddings are written by
capture; encrypted reference saving remains an explicit separate action.
No login/PAM/Howdy system configuration was changed.

## Verification and remaining validation

Tests cover dark-background signal, hair versus white glare, dim detailed faces,
low detail, stream frame/total-time/cancellation bounds and child reaping.
Explicit pinned-model tests run original face → blank → monochrome checkerboard
→ original face inside the same restricted worker, exercising the CLAHE fallback,
stale-state clearing and successful normal exit. The real camera test checks
continuous capture and verified cleanup without saving pixels.

A user-facing owner scan and separate-person/printed-photo/video trials remain
necessary. No consented held-out IR dataset or evaluated PAD model is available.
Neither Howdy's challenge nor our five-point geometry provides depth, dense 3D
shape or proof of spoof resistance. Authentication readiness therefore remains
false. The numerical acquisition heuristics and identity floor need empirical
calibration before any system-login use.
