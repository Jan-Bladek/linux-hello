# Camera compatibility

**Lenovo Yoga Slim 7 14IMH9: basic physical capture validated on 2026-10-11.**
On Fedora 44 KDE/Wayland, native UVC YUYV and GREY capture, conversion, release,
SIGINT cancellation and reopen passed. Full hardware qualification remains
incomplete; one initial RGB run rejected a driver-marked corrupted frame.
See the [executed results](validation.md#native-fedora-and-physical-cameras--2026-10-11).

Lenovo specifies an FHD camera with IR, an electronic shutter and a ToF sensor for this model family. This describes the product hardware; it does not establish Linux access to IR illumination or ToF data. Verify the actual camera module in the specific laptop before choosing a driver. [Lenovo PSREF](https://psref.lenovo.com/syspool/Sys/PDF/Yoga/Yoga_Slim_7_14IMH9/Yoga_Slim_7_14IMH9_Spec.PDF)

## Implemented support

| Capability | Current behavior |
|---|---|
| Enumeration | Enumerates `/dev/videoN`, queries per-node capabilities, formats and discrete/stepwise resolutions. Unreadable devices remain visible with an error. |
| Capture | Linux single-planar streaming V4L2, two requested mmap buffers, on-demand bounded worker. |
| Preview | GREY, packed YUYV and RGB24 (`RGB3`); negotiated dimensions and row padding are validated. YUYV uses BT.601 limited-range display conversion; color calibration is not provided. |
| IR discovery | Explicit IR/infrared name tokens or GREY support produce **hints only**. A monochrome image can come from a visible-light or virtual camera. |
| Shutdown | Cancellation is checked at intervals of up to 100 ms during polling; streams stop, mappings unmap and descriptors close. `stop().await` waits for worker teardown. A stuck kernel ioctl cannot be forcibly cancelled in user space. |
| Limits | Two queued frames, at most 32 MiB per raw/RGB frame, at most 60 seconds per session, at most 3,600 capture iterations. Slow consumers drop new frames rather than grow memory. |
| Reconnect | Disabled by default. With explicit opt-in, at most three reopen attempts at the same device path; driver/card/bus, sysfs path and device number must match the original selection. |
| Device identity | Prevents accidental selection changes, **not a cryptographic hardware trust path**. A malicious device can impersonate descriptors; a compromised kernel/session remains outside this guarantee. |
| Unsupported inputs | Metadata-only nodes, multi-planar streams, MJPEG-only cameras, Bayer/raw MIPI pipelines and proprietary depth formats fail explicitly. |
| Other platforms | Windows/macOS builds return `UnsupportedPlatform`; there is no fake camera fallback. |

All raw frame ownership stays inside the calling process; there is no camera-frame D-Bus endpoint. Frame `Debug` excludes pixels, and owned raw buffers are zeroed when dropped. Preview conversions create temporary RGB buffers that their caller must handle appropriately. This is not a guarantee against swap, process memory inspection or kernel buffer remanence. No frame is written to disk by this crate. No IR emitter controls or device firmware are modified.

The mmap adapter uses the generated ABI types and ioctl constants in `v4l` 0.14.0. Its small unsafe boundary is confined to `crates/hello-camera/src/linux.rs`, with ownership and bounds checks before copying camera buffers. It uses `File` cleanup rather than the upstream mmap stream destructor, which can panic for some cleanup errors. Reference APIs: [v4l 0.14.0](https://docs.rs/v4l/0.14.0/v4l/), [Linux streaming mmap](https://docs.kernel.org/userspace-api/media/v4l/mmap.html), [V4L2 capabilities](https://docs.kernel.org/userspace-api/media/v4l/vidioc-querycap.html).

## Lenovo qualification procedure

The reusable recorder can run the bounded capture/release/reopen test and save
its command result alongside local environment metadata:

```sh
python3 tools/qualify-linux.py
python3 tools/qualify-linux.py --verify --device /dev/videoN
```

Replace `/dev/videoN` with an explicitly selected capture node. Only `--device`
enables physical capture; preflight and `--verify` alone never start a stream.
The recorder also tests CLI SIGINT cancellation and reopen on that selected node.
It refuses root execution and existing report directories. It writes
logs and `report.json` under `work/`, without saving frames. A kernel-visible
camera absent from `/dev` is reported as an environment blocker, which can occur
inside an agent sandbox. Run qualification in the regular desktop terminal.
Passing the automated hardware test leaves the manual checks below pending.

Run as the logged-in desktop user on the Lenovo with its shutter open. Do not run the GUI as root or weaken `/dev/video*` permissions. Ensure another camera application is not holding the device. Install Fedora `v4l-utils` only if the local administrator wants the auxiliary diagnostic tool.

1. Record the Fedora release, kernel, BIOS version, laptop machine type and desktop session (GNOME/KDE/Hyprland). Do not publish serial numbers.
2. Inspect `v4l2-ctl --list-devices` and `v4l2-ctl --device /dev/videoN --all --list-formats-ext` for each candidate capture node. Match these with Linux Hello's camera list. Inspect USB/PCI identification locally if driver selection is unclear.
3. Confirm the selected node advertises a supported preview format. A separate IR node may advertise GREY while an RGB node advertises YUYV; neither naming nor format establishes liveness.
4. Run the real-hardware test explicitly from the workspace:

   ```sh
   LINUX_HELLO_CAMERA=/dev/videoN cargo test -p hello-camera --test hardware -- --ignored --nocapture
   ```

   This captures up to five frames, releases resources, then reopens and repeats. It validates buffers and RGB conversion without saving images. It is skipped during ordinary CI. It fails on missing permission, unsupported formats, changed identity, timeout or capture errors.
5. Use the application preview to inspect framing, shutter behavior and image quality. Cover the visible sensor separately from the suspected IR sensor and record observed behavior without drawing a liveness conclusion. Do not enable an emitter using an undocumented vendor control.
6. Cancel preview and verify the camera indicator turns off and another application can open the device. Repeat after a suspend/resume cycle. For an external test camera, disconnect during capture, reconnect and explicitly enumerate/select it again. The default session must fail on disconnect rather than switch camera.
7. Repeat capture/release on each intended desktop. Record pass/fail and exact software versions. Dark-room operation and spectral identification require a separate controlled physical evaluation; brightness alone does not prove IR.

Until these steps are executed on the actual laptop, the support status stays **unverified**, even when simulated and Linux CI tests pass. There is no validated presentation-attack detection in this camera subsystem, and a single IR image never authorizes authentication.

## Driver and access fallback

For a USB camera exposing ordinary V4L2 capture nodes, the kernel UVC driver is the expected route. If the device instead requires an Intel IPU/MIPI sensor pipeline, direct V4L2 may expose raw Bayer or multi-planar nodes that this preview cannot process. Fedora's documented IPU6 work uses a libcamera/software-ISP/PipeWire stack; support depends on the sensor and applications. Linux Hello currently has no libcamera or PipeWire camera backend, so successful camera use in Firefox does not imply that direct capture works here. [Fedora IPU6 camera support design](https://fedoraproject.org/wiki/Changes/IPU6_Camera_support)

For unsupported hardware, show the specific diagnostic and retain password authentication. A supported external V4L2 webcam can be selected for non-privileged preview tests. Do not install arbitrary out-of-tree drivers, grant world-readable device access, assume all Core Ultra systems use one camera stack, or treat a virtual camera as trusted biometric hardware. No hardware-specific driver installation is performed by this project.

## Qualification record

| System | Capture | IR/illumination | Cancellation/reopen | Desktop integration |
|---|---|---|---|---|
| Lenovo Yoga Slim 7 14IMH9, Fedora 44, kernel 7.2.9 | YUYV 640×480 (`video0`) and GREY 640×360 (`video2`) passed; one initial corrupted RGB frame rejected | Scoped MS face mode and alternating-frame response verified on `04f2:b7f5` revision `7204`; operator confirms illuminator and camera working; spectral measurement pending | SIGINT/SIGTERM emitter restoration and cooperative lease exclusion passed; suspend/unplug pending | KDE Wayland startup passed; operator confirms camera/illuminator operation following GUI instructions; GNOME and Hyprland pending |
| Simulated camera tests | Frame validation tested | Not simulated as a security property | Timeout/cancel/reconnect/identity rejection tested | Not applicable |

## Monochrome settling diagnosis

The latest 120-frame GREY series identified eight initial poor-quality frames;
112 later frames passed. The diagnostic face probe now waits for a quality-acceptable
candidate within a bounded series. Read-only control/USB inventories and the
unconfirmed emitter/spectrum/recognition gates are recorded in
[IR qualification](ir-qualification.md). This does not establish liveness.
