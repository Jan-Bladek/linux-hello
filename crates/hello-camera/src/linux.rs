//! Linux UAPI adapter. All pointer operations stay in this module. The generated
//! v4l bindings supply ABI types/constants; File and Mapping own kernel resources.
//! We avoid v4l 0.14's mmap Stream destructor (which can panic on STREAMOFF errors).

use crate::{
    CameraError, CameraSource, CaptureConfig, DeviceIdentity, DeviceInfo, FormatInfo, Frame,
    IrHint, MAX_FRAME_BYTES, PixelFormat, Resolution,
};
use std::{
    fs::{self, File, OpenOptions},
    io, mem,
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
    ptr::NonNull,
    time::Duration,
};
use v4l::{v4l_sys::*, v4l2::vidioc::*};

// The macro is private: every invocation must pair the request with its exact
// Linux UAPI argument type. It avoids a publicly callable untyped ioctl wrapper.
macro_rules! ioctl {
    ($file:expr, $request:expr, $arg:expr) => {{
        // SAFETY: each call below pairs a generated request with its documented
        // repr(C) UAPI argument; the exclusive pointer is valid for the syscall.
        let result = unsafe { libc::ioctl($file.as_raw_fd(), $request as _, $arg as *mut _) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }};
}
macro_rules! zeroed {
    ($ty:ty) => {{
        // SAFETY: only C UAPI structs containing integers, unions and raw pointers
        // use this macro. All-zero is valid and clears required reserved fields.
        unsafe { mem::zeroed::<$ty>() }
    }};
}

fn io_error(error: io::Error) -> CameraError {
    match error.raw_os_error() {
        Some(libc::ENODEV | libc::ENXIO | libc::ENOENT | libc::EPIPE) => CameraError::Disconnected,
        _ => CameraError::Io(error),
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes.split(|b| *b == 0).next().unwrap_or_default())
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect()
}

fn valid_path(path: &Path) -> bool {
    path.parent() == Some(Path::new("/dev"))
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("video"))
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) fn open_device(path: &Path) -> Result<File, CameraError> {
    if !valid_path(path) {
        return Err(CameraError::InvalidRequest(
            "select an enumerated /dev/videoN node",
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(io_error)?;
    if !file.metadata()?.file_type().is_char_device() {
        return Err(CameraError::InvalidRequest(
            "camera path is not a character device",
        ));
    }
    Ok(file)
}

pub(super) fn controls(path: &Path) -> Result<crate::ControlReport, CameraError> {
    let file = open_device(path)?;
    let mut controls = Vec::new();
    let mut previous = 0_u32;
    for _ in 0..128 {
        let mut query = zeroed!(v4l2_queryctrl);
        query.id = previous | V4L2_CTRL_FLAG_NEXT_CTRL;
        if let Err(error) = ioctl!(&file, VIDIOC_QUERYCTRL, &mut query) {
            return Ok(crate::ControlReport {
                controls,
                enumeration_complete: error.raw_os_error() == Some(libc::EINVAL),
                diagnostic: if error.raw_os_error() == Some(libc::EINVAL) {
                    None
                } else {
                    Some(error.to_string())
                },
            });
        }
        if query.id <= previous {
            return Err(CameraError::InvalidFrame(
                "control enumeration did not advance",
            ));
        }
        previous = query.id;
        let mut value = zeroed!(v4l2_control);
        value.id = query.id;
        // Simple integer/menu/boolean controls only. Compound, string and button
        // types require different UAPI arguments and are deliberately not queried.
        let scalar = [
            v4l2_ctrl_type_V4L2_CTRL_TYPE_INTEGER,
            v4l2_ctrl_type_V4L2_CTRL_TYPE_BOOLEAN,
            v4l2_ctrl_type_V4L2_CTRL_TYPE_MENU,
            v4l2_ctrl_type_V4L2_CTRL_TYPE_INTEGER_MENU,
        ]
        .contains(&query.type_);
        let result = if scalar && query.flags & V4L2_CTRL_FLAG_DISABLED == 0 {
            ioctl!(&file, VIDIOC_G_CTRL, &mut value)
        } else {
            Err(io::Error::from_raw_os_error(libc::ENOTSUP))
        };
        controls.push(crate::ControlInfo {
            id: query.id,
            name: text(&query.name),
            control_type: query.type_,
            minimum: query.minimum,
            maximum: query.maximum,
            step: query.step,
            default_value: query.default_value,
            flags: query.flags,
            current_value: result.as_ref().ok().map(|_| value.value),
            diagnostic: result.err().map(|e| e.to_string()),
        });
    }
    Ok(crate::ControlReport {
        controls,
        enumeration_complete: false,
        diagnostic: Some("control enumeration reached its 128-entry cap".into()),
    })
}

pub(super) fn enumerate() -> Result<Vec<DeviceInfo>, CameraError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir("/dev")? {
        let path = entry?.path();
        if valid_path(&path) {
            paths.push(path);
        }
        if paths.len() > 256 {
            return Err(CameraError::InvalidRequest(
                "video device count exceeds diagnostic limit",
            ));
        }
    }
    paths.sort();
    Ok(paths
        .into_iter()
        .map(
            |path| match open_device(&path).and_then(|file| describe(&file, &path)) {
                Ok(info) => info,
                Err(error) => DeviceInfo {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path,
                    driver: String::new(),
                    bus_info: String::new(),
                    identity: DeviceIdentity::default(),
                    formats: Vec::new(),
                    ir_hint: IrHint::Unknown,
                    capture_supported: false,
                    diagnostic: Some(error.to_string()),
                },
            },
        )
        .collect())
}

pub(super) fn describe(file: &File, path: &Path) -> Result<DeviceInfo, CameraError> {
    let mut caps = zeroed!(v4l2_capability);
    ioctl!(file, VIDIOC_QUERYCAP, &mut caps).map_err(io_error)?;
    let flags = if caps.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
        caps.device_caps
    } else {
        caps.capabilities
    };
    let capture_supported = flags & V4L2_CAP_VIDEO_CAPTURE != 0 && flags & V4L2_CAP_STREAMING != 0;
    let name = text(&caps.card);
    let driver = text(&caps.driver);
    let bus_info = text(&caps.bus_info);
    let sysfs = Path::new("/sys/class/video4linux")
        .join(path.file_name().ok_or(CameraError::DeviceChanged)?);
    let identity = DeviceIdentity {
        sysfs_path: sysfs.canonicalize().map_err(io_error)?,
        device_number: file.metadata()?.rdev(),
        driver: driver.clone(),
        card: name.clone(),
        bus_info: bus_info.clone(),
    };
    let (formats, diagnostic) = if capture_supported {
        match formats(file) {
            Ok(f) => (f, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        }
    } else {
        (Vec::new(),Some("Not a single-planar V4L2 streaming capture node (metadata and multi-planar nodes are not previewed)".into()))
    };
    let words = name.to_ascii_lowercase();
    let ir_hint = if words
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "ir" || w == "infrared")
    {
        IrHint::NameSuggestsInfrared
    } else if formats.iter().any(|f| f.pixel_format == PixelFormat::Grey) {
        IrHint::MonochromeFormat
    } else {
        IrHint::Unknown
    };
    Ok(DeviceInfo {
        path: path.into(),
        name,
        driver,
        bus_info,
        identity,
        formats,
        ir_hint,
        capture_supported,
        diagnostic,
    })
}

fn formats(file: &File) -> Result<Vec<FormatInfo>, CameraError> {
    let mut formats = Vec::new();
    for index in 0..128 {
        let mut desc = zeroed!(v4l2_fmtdesc);
        desc.index = index;
        desc.type_ = v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE;
        match ioctl!(file, VIDIOC_ENUM_FMT, &mut desc) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(libc::EINVAL) => return Ok(formats),
            Err(e) => return Err(io_error(e)),
        }
        formats.push(FormatInfo {
            pixel_format: PixelFormat::from_code(desc.pixelformat),
            description: text(&desc.description),
            resolutions: resolutions(file, desc.pixelformat)?,
        });
    }
    Err(CameraError::InvalidFrame(
        "driver advertises too many pixel formats",
    ))
}

fn resolutions(file: &File, code: u32) -> Result<Vec<Resolution>, CameraError> {
    let mut sizes = Vec::new();
    for index in 0..256 {
        let mut desc = zeroed!(v4l2_frmsizeenum);
        desc.index = index;
        desc.pixel_format = code;
        match ioctl!(file, VIDIOC_ENUM_FRAMESIZES, &mut desc) {
            Ok(()) => {}
            Err(e)
                if e.raw_os_error() == Some(libc::EINVAL)
                    || e.raw_os_error() == Some(libc::ENOTTY) =>
            {
                return Ok(sizes);
            }
            Err(e) => return Err(io_error(e)),
        }
        match desc.type_ {
            kind if kind == v4l2_frmsizetypes_V4L2_FRMSIZE_TYPE_DISCRETE => {
                // SAFETY: the driver selected the DISCRETE union member above.
                let size = unsafe { desc.__bindgen_anon_1.discrete };
                sizes.push(Resolution::Discrete {
                    width: size.width,
                    height: size.height,
                });
            }
            kind if kind == v4l2_frmsizetypes_V4L2_FRMSIZE_TYPE_STEPWISE
                || kind == v4l2_frmsizetypes_V4L2_FRMSIZE_TYPE_CONTINUOUS =>
            {
                // SAFETY: the driver selected STEPWISE/CONTINUOUS above.
                let size = unsafe { desc.__bindgen_anon_1.stepwise };
                sizes.push(Resolution::Stepwise {
                    min_width: size.min_width,
                    max_width: size.max_width,
                    step_width: size.step_width,
                    min_height: size.min_height,
                    max_height: size.max_height,
                    step_height: size.step_height,
                });
                return Ok(sizes);
            }
            _ => return Err(CameraError::InvalidFrame("unknown frame-size descriptor")),
        }
    }
    Err(CameraError::InvalidFrame(
        "driver advertises too many resolutions",
    ))
}

struct Mapping {
    ptr: NonNull<u8>,
    len: usize,
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this uniquely owned region came from mmap and is released once.
        // Errors during disconnect cleanup must never panic or abort the process.
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast(), self.len);
        }
    }
}

pub(super) struct LinuxCamera {
    file: File,
    maps: Vec<Mapping>,
    width: u32,
    height: u32,
    stride: u32,
    pixel_format: PixelFormat,
    streaming: bool,
}

fn buffer(index: u32) -> v4l2_buffer {
    let mut value = zeroed!(v4l2_buffer);
    value.index = index;
    value.type_ = v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE;
    value.memory = v4l2_memory_V4L2_MEMORY_MMAP;
    value
}

impl LinuxCamera {
    pub(super) fn open(config: &CaptureConfig) -> Result<Self, CameraError> {
        let file = open_device(&config.device.path)?;
        // Re-enumeration on the newly opened descriptor binds this capture to the
        // selected camera. Never scan for a similar name or silently change node.
        let current = describe(&file, &config.device.path)?;
        if current.identity != config.device.identity {
            return Err(CameraError::DeviceChanged);
        }
        if !current.capture_supported
            || !current
                .formats
                .iter()
                .any(|f| f.pixel_format == config.pixel_format)
        {
            return Err(CameraError::UnsupportedFormat);
        }
        let code = config
            .pixel_format
            .fourcc()
            .as_bytes()
            .try_into()
            .map(u32::from_le_bytes)
            .map_err(|_| CameraError::UnsupportedFormat)?;
        let mut format = zeroed!(v4l2_format);
        format.type_ = v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE;
        let mut pixels = zeroed!(v4l2_pix_format);
        pixels.width = config.width;
        pixels.height = config.height;
        pixels.pixelformat = code;
        pixels.field = v4l2_field_V4L2_FIELD_NONE;
        format.fmt.pix = pixels;
        ioctl!(&file, VIDIOC_S_FMT, &mut format).map_err(io_error)?;
        // SAFETY: type_ is single-planar VIDEO_CAPTURE; pix is its UAPI member.
        let negotiated = unsafe { format.fmt.pix };
        if negotiated.pixelformat != code {
            return Err(CameraError::UnsupportedFormat);
        }
        let pixel_format = PixelFormat::from_code(negotiated.pixelformat);
        let row = negotiated
            .width
            .checked_mul(pixel_format.bytes_per_pixel()? as u32)
            .ok_or(CameraError::InvalidFrame("negotiated width overflow"))?;
        let stride = negotiated.bytesperline.max(row);
        let required = (stride as usize)
            .checked_mul(negotiated.height as usize)
            .filter(|n| *n > 0 && *n <= MAX_FRAME_BYTES)
            .ok_or(CameraError::InvalidFrame(
                "negotiated allocation exceeds memory limit",
            ))?;
        if negotiated.width == 0
            || negotiated.height == 0
            || negotiated.width > 8192
            || negotiated.height > 8192
            || negotiated.sizeimage as usize > MAX_FRAME_BYTES
            || (negotiated.sizeimage as usize) < required
        {
            return Err(CameraError::InvalidFrame("invalid negotiated format"));
        }
        if pixel_format == PixelFormat::Yuyv && !negotiated.width.is_multiple_of(2) {
            return Err(CameraError::InvalidFrame("odd YUYV width"));
        }
        let mut camera = Self {
            file,
            maps: Vec::new(),
            width: negotiated.width,
            height: negotiated.height,
            stride,
            pixel_format,
            streaming: false,
        };
        let mut request = zeroed!(v4l2_requestbuffers);
        request.count = 2;
        request.type_ = v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE;
        request.memory = v4l2_memory_V4L2_MEMORY_MMAP;
        ioctl!(&camera.file, VIDIOC_REQBUFS, &mut request).map_err(io_error)?;
        if request.count == 0 || request.count > 8 {
            return Err(CameraError::InvalidFrame(
                "driver returned invalid buffer count",
            ));
        }
        for index in 0..request.count {
            let mut desc = buffer(index);
            ioctl!(&camera.file, VIDIOC_QUERYBUF, &mut desc).map_err(io_error)?;
            let len = desc.length as usize;
            if len < required || len > MAX_FRAME_BYTES {
                return Err(CameraError::InvalidFrame("invalid mapped buffer length"));
            }
            // SAFETY: memory is MMAP; offset is the correct union member. The
            // driver supplied the offset and bounded length for this descriptor.
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    camera.file.as_raw_fd(),
                    desc.m.offset as libc::off_t,
                )
            };
            if ptr == libc::MAP_FAILED {
                return Err(io_error(io::Error::last_os_error()));
            }
            let Some(ptr) = NonNull::new(ptr.cast::<u8>()) else {
                // SAFETY: an address-zero successful mapping still must be unmapped.
                unsafe {
                    libc::munmap(ptr, len);
                }
                return Err(CameraError::InvalidFrame("null camera mapping"));
            };
            camera.maps.push(Mapping { ptr, len });
            ioctl!(&camera.file, VIDIOC_QBUF, &mut desc).map_err(io_error)?;
        }
        let mut kind = v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE;
        ioctl!(&camera.file, VIDIOC_STREAMON, &mut kind).map_err(io_error)?;
        camera.streaming = true;
        Ok(camera)
    }
}

impl CameraSource for LinuxCamera {
    fn stop(&mut self) -> Result<(), CameraError> {
        if self.streaming {
            let mut kind = v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE;
            ioctl!(&self.file, VIDIOC_STREAMOFF, &mut kind).map_err(io_error)?;
            self.streaming = false;
        }
        self.maps.clear();
        Ok(())
    }
    fn next(&mut self, wait: Duration) -> Result<Option<Frame>, CameraError> {
        let mut poll = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout = wait.as_millis().clamp(1, 100) as i32;
        // SAFETY: poll points to one initialized, valid pollfd for this call.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(io_error(error));
        }
        if ready == 0 {
            return Ok(None);
        }
        if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(CameraError::Disconnected);
        }
        let mut desc = buffer(0);
        match ioctl!(&self.file, VIDIOC_DQBUF, &mut desc) {
            Ok(()) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::Interrupted =>
            {
                return Ok(None);
            }
            Err(e) => return Err(io_error(e)),
        }
        let map = self
            .maps
            .get(desc.index as usize)
            .ok_or(CameraError::InvalidFrame(
                "driver returned out-of-range buffer index",
            ))?;
        if desc.flags & V4L2_BUF_FLAG_ERROR != 0 {
            return Err(CameraError::InvalidFrame("driver marked frame corrupted"));
        }
        let len = desc.bytesused as usize;
        if len == 0 || len > map.len {
            return Err(CameraError::InvalidFrame(
                "driver bytesused exceeds mapping",
            ));
        }
        // SAFETY: successful DQBUF transferred this mapped buffer to userspace;
        // checked bytesused stays in its unique mapping. Copy before re-queueing.
        let data = unsafe { std::slice::from_raw_parts(map.ptr.as_ptr(), len) }.to_vec();
        let frame = Frame {
            width: self.width,
            height: self.height,
            stride: self.stride,
            pixel_format: self.pixel_format.clone(),
            sequence: desc.sequence,
            data,
        };
        frame.validate()?;
        ioctl!(&self.file, VIDIOC_QBUF, &mut desc).map_err(io_error)?;
        Ok(Some(frame))
    }
}

impl Drop for LinuxCamera {
    fn drop(&mut self) {
        if self.streaming {
            let mut kind = v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE;
            let _ = ioctl!(&self.file, VIDIOC_STREAMOFF, &mut kind);
        }
        // Unmap before closing the fd. File Drop closes it even if ioctl failed;
        // closing releases remaining queued/requested kernel buffers.
        self.maps.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_paths_reject_traversal_symlinks_and_non_video_names() {
        for path in [
            "/tmp/video0",
            "/dev/video",
            "/dev/video0/../video1",
            "/dev/video-1",
            "/dev/v4l/by-id/test",
        ] {
            assert!(!valid_path(Path::new(path)));
        }
        assert!(valid_path(Path::new("/dev/video12")));
    }
    #[test]
    fn diagnostic_strings_remove_controls_and_invalid_utf8() {
        assert_eq!(text(b"IR\nCamera\0ignored"), "IRCamera");
        assert_eq!(text(&[255, 0]), "\u{fffd}");
    }
}
