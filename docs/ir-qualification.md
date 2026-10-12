# Physical monochrome/IR qualification — 2026-10-11

Host: Lenovo Yoga Slim 7 14IMH9, Fedora 44 KDE/Wayland, Chicony USB `04f2:b7f5`,
UVC interface `3-9:1.2`, `/dev/video2`, GREY 640×360. Linux identifies the device
as `Integrated Camera: Integrated I`. Device naming, GREY and product-family
specifications remain IR hints, not wavelength or emitter proof.

## Current emitter control — implemented and tested

The operator subsequently followed the GUI check instructions and reported:
“pięknie działa iliminator i kamera”. This confirms observed illuminator and camera
operation on this laptop. The report did not separately describe blink timing or
whether the light went off after Stop, so those visual details are not inferred.
Register restoration remains supported by the hardware tests below.

The camera exposes Microsoft Face Authentication mode on extension unit 14,
selector 6, GUID `0f3f95dc-2632-4c4e-92c9-a04782f43bc8`. The firmware reports
GET_LEN=9, GET_INFO=3, GET_MAX=`01 03 03 00 00 00 00 00 00`, and initial
GET_CUR=`01 03 01 00 00 00 00 00 00`. This is one entry for streaming interface
3, with general-purpose D0 and face-mode D1 supported. Selecting
`01 03 02 00 00 00 00 00 00` succeeded and read back identically. Restoring the
initial bytes succeeded and read back identically. This follows the
[Microsoft Face Authentication control specification](https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/uvc-extensions-1-5#2226-face-authentication-control).
No undocumented Realtek intensity bytes were written.

Native `IrEmitterLease` now enables this mode for an explicit capture session.
CLI `capture`, optional `camera-quality` and `face-probe` accept `--ir-emitter`.
GUI Camera has an opt-in **Use IR illumination** checkbox. Defaults leave the
control untouched. No root, extra dependency, udev rule, boot service, external
emitter executable or PAM change is required. Firmware mode is reapplied on every
requested session, rather than assuming settings survive a boot/resume.

The adapter binds the selected opened device to its enumerated identity and
accepts only Linux x86_64, Chicony `04f2:b7f5`, revision `7204`, USB control
interface 02 with GREY capture. It finds the Microsoft unit from bounded USB
descriptors (64 KiB), requires the selector bitmap, exact length/info and reviewed
one-entry capability layout, preserves the original setting, writes only selector
6 and validates readback. RGB/metadata/unsupported hardware fails before writes.
An advisory exclusive flock prevents concurrent cooperating Linux Hello leases;
other applications must still coordinate their camera use.

Capture stops and joins before explicit restoration. Restoration and readback
must succeed before CLI/GUI reports completion. Drop retries restoration after
ordinary errors or failed setup. CLI retains SIGINT/SIGTERM handlers and restores
on either signal. Drop cannot execute after SIGKILL, process abort, power loss or
device removal; those cases cannot promise restoration. A kernel USB ioctl may
delay cleanup until its driver timeout. Disconnect/readback errors remain failures.

Five successive 90-frame runs toggled normal → face → normal → face → normal.
All restored the original bytes. Discarding the first 30 frames for this descriptive
summary, parity medians were:

| Run | Mode | Odd-frame mean-intensity median | Even-frame median |
|---|---|---:|---:|
| 0 | Normal | 0.063140 | 0.063153 |
| 1 | Face/IR | 0.097555 | 0.001482 |
| 2 | Normal restored | 0.066803 | 0.066683 |
| 3 | Face/IR | 0.099727 | 0.001847 |
| 4 | Normal restored | 0.071805 | 0.071782 |

Regular alternating bright/dark output appeared only in face mode and disappeared
after restoration. This is reproducible functional evidence for the activation
path, consistent with strobed IR illumination. Those automated reports preceded
the operator's confirmation above; their `emitter_confirmed`/`spectrum_confirmed`
fields remain false as originally recorded. Controlled spectral/illumination
measurements are still outstanding, while
`ir_face_mode_requested` and `ir_face_mode_restored` report the verified operation.
No claim about PAD, face accuracy or production authentication follows from this.

Separate live tests verified normal completion, SIGINT and SIGTERM restoration,
rejection of concurrent leases and rejection of `/dev/video0`. The explicitly
ignored Rust hardware test verifies capture/stop and Drop after invalid capture
setup. All checks ran as the normal desktop user. Logs are
`work/inference/emitter-ms-trial.json`, `emitter-native-verification.json`,
`emitter-native-quality-*.json` and `emitter-candidate-registers.json`. Only
registers and statistics were saved, never camera pixels or embeddings.

Face-probe allows 90 candidates in requested IR mode because alternating dark
frames must be rejected by the unchanged quality policy; normal mode retains 30.
Brightness/parity does not become an authentication or trusted-illumination verdict.

## Earlier settling diagnosis (normal mode)

A 120-frame series completed without saving pixels. Frames 1–8 failed the unchanged
diagnostic quality policy. Frames 9–120 (112 frames) passed. This explains why the
previous fixed fifth-frame probe rejected a dark image: the sensor had not settled.
The probe now skips the first four frames and selects the first acceptable candidate
within at most 30 delivered frames. If none passes it retains a rejected candidate
for the normal failure report. No threshold was lowered and no enhancement is
applied to disguise insufficient quality.

| Whole-series statistic | Minimum | Median | Maximum |
|---|---:|---:|---:|
| Mean intensity | 0.000575 | 0.105422 | 0.105868 |
| Contrast | 0.001516 | 0.095041 | 0.095282 |
| Edge energy | 0.000707 | 0.026101 | 0.026250 |

The following settled-frame probe passed image-quality checks, but YuNet returned
zero faces, so inference was rejected. Face position/lighting were not independently
confirmed in that run. This is not a demonstrated successful NIR recognition result.

The read-only standard V4L2 inventory completed and exposed control-class headers,
Region of Interest Auto Controls and Privacy. Privacy was 0. No exposure or emitter
switch was exposed through this inventory. These are properties of the current
driver/module, not proof that firmware has no additional controls. The tool issues
only `QUERYCTRL`/`G_CTRL`, never `S_CTRL` or UVC extension-unit writes.

The initial read-only USB descriptor capture identified extension-unit GUIDs before
the successful face-mode experiment above. At that earlier stage no emitter
controls were changed. The external tool was then
reviewed as described below; it remains uninstalled and is not a dependency.

## Extension-unit investigation

Added `tools/inspect-uvc-controls.py`, a Python standard-library diagnostic that
selects the exact device's sysfs USB control interface and parses its declared
extension-unit bitmaps. It opens the matching character device with
`O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC`, checks its major/minor against sysfs,
and sends only `GET_LEN` and `GET_INFO`. It never reads current control payloads,
sets controls, maps controls or starts capture. Descriptor parsing is bounded to
64 KiB and 128 selectors; malformed lengths and duplicate unit IDs are rejected.
Failed queries are preserved in JSON and cause exit status 1. Partial results
must not be read as proof that missing controls do not exist. Wrap the command
in `timeout` to bound the whole invocation if USB requests stall.

On the actual `/dev/video2`, all 18 queries completed without elevated privileges.
The firmware declares nine selectors; eight advertise GET+SET, one GET only:

| Unit | GUID family | Selector | Length (bytes) | GET_INFO | Advertised support |
|---|---|---:|---:|---:|---|
| 10 | `1229a78c-47b4-4094-b0ce-db07386fb938` | 10 | 8 | 3 | GET+SET |
| 10 | same | 11 | 8 | 3 | GET+SET |
| 13 | `26b8105a-0713-4870-979d-da79444bb68e` | 14 | 2 | 3 | GET+SET |
| 13 | same | 15 | 1 | 1 | GET |
| 13 | same | 16 | 1 | 3 | GET+SET |
| 14 | `0f3f95dc-2632-4c4e-92c9-a04782f43bc8` | 2 | 15 | 19 | GET+SET, asynchronous bit |
| 14 | same | 3 | 11 | 19 | GET+SET, asynchronous bit |
| 14 | same | 6 | 9 | 3 | GET+SET |
| 14 | same | 9 | 4 | 3 | GET+SET |

Unit 11 has an empty control bitmap. RGB `/dev/video0` also completed its
metadata inventory (ten declared selectors), independently of capture.
The GET/SET support-bit interpretation follows the
[Linux UVC driver documentation](https://docs.kernel.org/userspace-api/media/drivers/uvcvideo.html).
The flags describe firmware claims, not verified write semantics.

Reviewed `linux-enable-ir-emitter` at commit
`2411c9c3199f80a82169df07c39e5b45dd336e3d`. Its
[control discovery](https://github.com/EmixamPP/linux-enable-ir-emitter/blob/2411c9c3199f80a82169df07c39e5b45dd336e3d/src/video/uvc/device.rs)
scans unit/selector combinations and writes the current value back to test
writability. Its
[configuration loop](https://github.com/EmixamPP/linux-enable-ir-emitter/blob/2411c9c3199f80a82169df07c39e5b45dd336e3d/src/video/ir/enabler.rs)
then increments payload bytes and tests the effect. Neither path establishes
an exact `04f2:b7f5` firmware command from a verified mapping. They were not run.

A [first-person Linux UVC report for a Lenovo T490s, `04f2:b67c`](https://sourceforge.net/p/linux-uvc/mailman/message/36798879/)
identifies the `26b8105a` GUID, selector 14 and a two-byte emitter command.
Our unit 13 matches that GUID/selector/size, making it a research candidate.
This is an inference across different camera models, not validation for `b7f5`;
the other camera's payload was not applied here.

The [Microsoft UVC extension specification](https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/uvc-extensions-1-5)
assigns IR Torch to selector 10 and Face Authentication to selector 6. Unit 14
does not advertise selector 10, so no standard Microsoft IR Torch control is
declared on this interface. Selector 10 on unit 10 belongs to a different GUID
and cannot be assigned Microsoft semantics by its number or length alone.
Selector 6 supplied the successful documented face-mode activation path above;
its presence and successful control still do not establish authentication or PAD.

## Repeat without storing frames

```sh
cargo build --locked --release -p hello-cli --features face-opencv
target/release/hello capture --device /dev/video2 --frames 90 --timeout 15 --ir-emitter
target/release/hello camera-quality --device /dev/video2 --frames 90 --ir-emitter
target/release/hello face-probe --device /dev/video2 --models work/inference/models --ir-emitter
timeout 30s python3 tools/inspect-uvc-controls.py --device /dev/video2
target/debug/hello camera-controls --device /dev/video2
target/debug/hello camera-quality --device /dev/video2 --frames 120
target/debug/hello face-probe --device /dev/video2 --models work/inference/models
```

The controls command does not start capture. Quality and face-probe explicitly
start bounded capture. Quality JSON reports frame sequence, dimensions/format and
statistics, with readiness/spectrum/emitter flags false. Quality success means the
series was acquired, not that it is suitable for authentication. Capture errors,
cancellation and empty streams remain failures. Per-frame statistics can reveal
ambient conditions, so retain reports only as intended local diagnostics.

`work/inference/ir-quality-120.json`, `ir-quality-summary.json`, `ir-controls.json`,
`ir-settled-probe.json`, `ir-xu-metadata.json`, `rgb-xu-metadata.json` and read-only
USB descriptors hold the captured evidence.
No user camera pixels or embeddings were stored.

## Still required for full IR qualification

Confirm a face is actually in-frame and the shutter is open; collect labeled runs
with known illumination and distances. Verify NIR response using a controlled,
known light source and verify emitter state/indicator behavior with a reviewed
module-specific control. Repeat quality and recognition across light/dark,
glasses, pose and time-separated sessions. Check shutter, suspend/resume and
restoration after suspend/resume or device reset. Compare RGB and IR failure-to-acquire and recognition rates
using the consented protocol in [model evaluation](model-evaluation.md).
A monochrome quality pass, IR illumination or a detected face supplies no PAD.
