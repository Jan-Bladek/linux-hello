//! Scoped, opt-in MS Face Authentication mode for the tested Chicony firmware.
//! This controls illumination/capture mode, never supplies an authentication verdict.
use crate::{CameraError, DeviceInfo};

pub struct IrEmitterLease {
    #[cfg(target_os = "linux")]
    inner: linux::Lease,
}
impl IrEmitterLease {
    pub fn acquire(device: &DeviceInfo) -> Result<Self, CameraError> {
        #[cfg(target_os = "linux")]
        {
            Ok(Self {
                inner: linux::Lease::acquire(device)?,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = device;
            Err(CameraError::UnsupportedPlatform)
        }
    }
    /// Reassert the allowlisted face-mode payload after the video stream has started.
    /// Some firmware resets capture mode during stream initialization.
    pub fn rearm_streaming(&self) -> Result<(), CameraError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.rearm_streaming()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(CameraError::UnsupportedPlatform)
        }
    }
    /// Verify restoration before declaring capture complete. Drop retries on failure.
    pub fn restore(self) -> Result<(), CameraError> {
        #[cfg(target_os = "linux")]
        {
            let mut inner = self.inner;
            inner.restore()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(CameraError::UnsupportedPlatform)
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs::{self, File},
        io::{self, Read},
        mem,
        os::fd::AsRawFd,
    };
    const GUID: [u8; 16] = [
        0xdc, 0x95, 0x3f, 0x0f, 0x32, 0x26, 0x4e, 0x4c, 0x92, 0xc9, 0xa0, 0x47, 0x82, 0xf4, 0x3b,
        0xc8,
    ];
    #[repr(C)]
    struct Query {
        unit: u8,
        selector: u8,
        query: u8,
        size: u16,
        data: *mut u8,
    }
    const REQUEST: libc::c_ulong = (3 << 30)
        | ((mem::size_of::<Query>() as libc::c_ulong) << 16)
        | ((b'u' as libc::c_ulong) << 8)
        | 0x21;
    fn query(file: &File, unit: u8, code: u8, data: &mut [u8]) -> Result<(), CameraError> {
        // This private adapter uses only documented GETs and selector-6 SET_CUR.
        // SAFETY: this C struct contains only integers and a nullable pointer;
        // zeroing also clears ABI padding before the kernel copies the argument.
        let mut q: Query = unsafe { mem::zeroed() };
        q.unit = unit;
        q.selector = 6;
        q.query = code;
        q.size = data.len() as u16;
        q.data = data.as_mut_ptr();
        // SAFETY: repr(C) matches linux/uvcvideo.h; bounded buffer and exclusive
        // argument remain live for this synchronous ioctl. Request size uses ABI.
        if unsafe { libc::ioctl(file.as_raw_fd(), REQUEST, &mut q) } < 0 {
            return Err(CameraError::Io(io::Error::last_os_error()));
        }
        Ok(())
    }
    fn invalid() -> CameraError {
        CameraError::InvalidRequest(
            "IR emitter requires the tested 04f2:b7f5 firmware 7204, IR interface 02 and Microsoft face-mode control",
        )
    }
    fn find_unit(data: &[u8], interface: u8) -> Result<u8, CameraError> {
        let mut offset = 0;
        let mut active = false;
        let mut found = None;
        while offset < data.len() {
            let header = data.get(offset..offset + 2).ok_or_else(invalid)?;
            let size = header[0] as usize;
            if size < 2 {
                return Err(invalid());
            }
            let d = data.get(offset..offset + size).ok_or_else(invalid)?;
            match d[1] {
                2 => active = false,
                4 => {
                    if size < 9 {
                        return Err(invalid());
                    }
                    active = d[2] == interface && d[3] == 0 && d[5..7] == [14, 1];
                }
                0x24 if active && size >= 3 && d[2] == 6 => {
                    if size < 24 {
                        return Err(invalid());
                    }
                    let bitmap_offset = 22 + d[21] as usize;
                    let bitmap_len = *d.get(bitmap_offset).ok_or_else(invalid)? as usize;
                    if bitmap_offset + bitmap_len + 2 != size {
                        return Err(invalid());
                    }
                    if d[4..20] == GUID {
                        if found.is_some()
                            || d[3] == 0
                            || bitmap_len == 0
                            || d[bitmap_offset + 1] & 0x20 == 0
                        {
                            return Err(invalid());
                        }
                        found = Some(d[3]);
                    }
                }
                _ => {}
            }
            offset += size;
        }
        found.ok_or_else(invalid)
    }
    fn enabled(current: [u8; 9], maximum: [u8; 9]) -> Result<[u8; 9], CameraError> {
        // One entry: streaming interface 3, little-endian u64 mode flags.
        // D0=general-purpose; D1=face mode. Preserve the supplied entry shape.
        if current[0..2] != [1, 3]
            || maximum != [1, 3, 3, 0, 0, 0, 0, 0, 0]
            || !matches!(current[2], 1 | 2)
            || current[3..] != [0; 6]
        {
            return Err(invalid());
        }
        let mut value = current;
        value[2] = 2;
        Ok(value)
    }
    pub(super) struct Lease {
        file: File,
        unit: u8,
        original: [u8; 9],
        active: bool,
    }
    impl Lease {
        pub(super) fn acquire(device: &DeviceInfo) -> Result<Self, CameraError> {
            if std::env::consts::ARCH != "x86_64"
                || device.driver != "uvcvideo"
                || !device.capture_supported
                || !device
                    .formats
                    .iter()
                    .any(|f| f.pixel_format == crate::PixelFormat::Grey)
            {
                return Err(invalid());
            }
            let file = crate::linux::open_device(&device.path)?;
            let current = crate::linux::describe(&file, &device.path)?;
            if current.identity != device.identity {
                return Err(CameraError::DeviceChanged);
            }
            // Advisory ownership across cooperating Linux Hello processes. Other
            // applications need coordination; this is not kernel exclusivity.
            // SAFETY: flock receives the valid owned fd and integer flags only.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
                return Err(CameraError::Io(io::Error::last_os_error()));
            }
            let interface = fs::canonicalize(device.identity.sysfs_path.join("device"))?;
            let usb = interface.parent().ok_or_else(invalid)?;
            for (field, expected) in [
                ("idVendor", "04f2"),
                ("idProduct", "b7f5"),
                ("bcdDevice", "7204"),
            ] {
                if fs::read_to_string(usb.join(field))?.trim() != expected {
                    return Err(invalid());
                }
            }
            if fs::read_to_string(interface.join("bInterfaceNumber"))?.trim() != "02" {
                return Err(invalid());
            }
            let mut descriptors = Vec::new();
            File::open(usb.join("descriptors"))?
                .take(65537)
                .read_to_end(&mut descriptors)?;
            if descriptors.len() > 65536 {
                return Err(invalid());
            }
            let unit = find_unit(&descriptors, 2)?;
            let mut length = [0; 2];
            query(&file, unit, 0x85, &mut length)?;
            let mut info = [0; 1];
            query(&file, unit, 0x86, &mut info)?;
            if u16::from_le_bytes(length) != 9 || info != [3] {
                return Err(invalid());
            }
            let mut maximum = [0; 9];
            query(&file, unit, 0x83, &mut maximum)?;
            let mut original = [0; 9];
            query(&file, unit, 0x81, &mut original)?;
            let mut value = enabled(original, maximum)?;
            // Arm restoration before writing so a partially successful SET or
            // failed readback cannot leave an unowned configuration behind.
            let mut lease = Self {
                file,
                unit,
                original,
                active: true,
            };
            if value != original {
                query(&lease.file, unit, 1, &mut value)?;
            }
            let mut readback = [0; 9];
            query(&lease.file, unit, 0x81, &mut readback)?;
            if readback != value {
                lease.restore()?;
                return Err(CameraError::InvalidFrame("IR face-mode readback differs"));
            }
            Ok(lease)
        }
        pub(super) fn rearm_streaming(&self) -> Result<(), CameraError> {
            let mut maximum = [0; 9];
            query(&self.file, self.unit, 0x83, &mut maximum)?;
            let mut current = [0; 9];
            query(&self.file, self.unit, 0x81, &mut current)?;
            let mut value = enabled(current, maximum)?;
            query(&self.file, self.unit, 1, &mut value)?;
            let mut readback = [0; 9];
            query(&self.file, self.unit, 0x81, &mut readback)?;
            if readback != value {
                return Err(CameraError::InvalidFrame(
                    "IR streaming-mode readback differs",
                ));
            }
            Ok(())
        }
        pub(super) fn restore(&mut self) -> Result<(), CameraError> {
            if self.active {
                let mut original = self.original;
                query(&self.file, self.unit, 1, &mut original)?;
                let mut readback = [0; 9];
                query(&self.file, self.unit, 0x81, &mut readback)?;
                if readback != self.original {
                    return Err(CameraError::InvalidFrame(
                        "IR face-mode restoration differs",
                    ));
                }
                self.active = false;
            }
            Ok(())
        }
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            if let Err(error) = self.restore() {
                eprintln!("IR emitter state restoration failed: {error}");
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn descriptor(interface: u8, alternate: u8) -> Vec<u8> {
            let mut d = vec![9, 4, interface, alternate, 0, 14, 1, 0, 0];
            let mut xu = vec![26, 0x24, 6, 14];
            xu.extend(GUID);
            xu.extend([1, 1, 10, 1, 0x20, 0]);
            d.extend(xu);
            d
        }
        #[test]
        fn control_discovery_is_guid_interface_and_selector_bound() {
            assert_eq!(find_unit(&descriptor(2, 0), 2).unwrap(), 14);
            for d in [
                descriptor(0, 0),
                descriptor(2, 1),
                vec![0, 4],
                vec![26, 0x24, 6],
                {
                    let mut d = descriptor(2, 0);
                    d[13] ^= 1;
                    d
                },
                {
                    let mut d = descriptor(2, 0);
                    d[33] = 0;
                    d
                },
                {
                    let mut d = descriptor(2, 0);
                    d.extend(descriptor(2, 0));
                    d
                },
            ] {
                assert!(find_unit(&d, 2).is_err());
            }
        }
        #[test]
        fn face_mode_accepts_only_the_reviewed_layout_and_capability() {
            let normal = [1, 3, 1, 0, 0, 0, 0, 0, 0];
            let max = [1, 3, 3, 0, 0, 0, 0, 0, 0];
            assert_eq!(enabled(normal, max).unwrap(), [1, 3, 2, 0, 0, 0, 0, 0, 0]);
            for index in 0..9 {
                let mut changed = max;
                changed[index] ^= 4;
                assert!(enabled(normal, changed).is_err());
            }
            for changed in [
                [1, 1, 1, 0, 0, 0, 0, 0, 0],
                [1, 3, 3, 0, 0, 0, 0, 0, 0],
                [1, 3, 1, 0, 0, 0, 0, 0, 1],
            ] {
                assert!(enabled(changed, max).is_err());
            }
        }

        #[tokio::test]
        #[ignore = "requires LINUX_HELLO_CAMERA and explicitly authorized IR mode writes/capture"]
        async fn selected_ir_mode_captures_and_restores_on_stop_and_drop() {
            let path = std::path::PathBuf::from(
                std::env::var("LINUX_HELLO_CAMERA").expect("select the tested IR device"),
            );
            let device = crate::enumerate()
                .unwrap()
                .into_iter()
                .find(|d| d.path == path)
                .unwrap();
            let observer = crate::linux::open_device(&path).unwrap();
            let lease = IrEmitterLease::acquire(&device).unwrap();
            let original = lease.inner.original;
            let unit = lease.inner.unit;
            let mut current = [0; 9];
            query(&observer, unit, 0x81, &mut current).unwrap();
            assert_eq!(current, [1, 3, 2, 0, 0, 0, 0, 0, 0]);
            let mut config = crate::CaptureConfig::preview(device.clone()).unwrap();
            config.max_frames = 12;
            let mut camera = crate::CameraSession::start(config).unwrap();
            let mut count = 0;
            while camera.next_frame().await.unwrap().is_some() {
                count += 1;
            }
            camera.stop().await.unwrap();
            lease.restore().unwrap();
            assert_eq!(count, 12);
            query(&observer, unit, 0x81, &mut current).unwrap();
            assert_eq!(current, original);
            let lease = IrEmitterLease::acquire(&device).unwrap();
            let mut bad = crate::CaptureConfig::preview(device).unwrap();
            bad.width = 0;
            assert!(crate::CameraSession::start(bad).is_err());
            drop(lease);
            query(&observer, unit, 0x81, &mut current).unwrap();
            assert_eq!(current, original);
        }
    }
}
