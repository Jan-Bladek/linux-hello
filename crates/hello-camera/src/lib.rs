//! Bounded, on-demand camera diagnostics. Camera pixels are not evidence of liveness.
//! No frames are persisted by this crate. Linux device access is confined to `linux`.

use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "linux", test))]
use std::time::Instant;
use std::{
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroize;

#[cfg(target_os = "linux")]
mod linux;

mod emitter;
pub use emitter::IrEmitterLease;

const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
#[cfg(any(target_os = "linux", test))]
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Error)]
pub enum CameraError {
    #[error("camera capture requires Linux V4L2")]
    UnsupportedPlatform,
    #[error("invalid camera request: {0}")]
    InvalidRequest(&'static str),
    #[error("camera does not support this preview pixel format")]
    UnsupportedFormat,
    #[error("camera identity changed; enumerate and select the device again")]
    DeviceChanged,
    #[error("camera disconnected")]
    Disconnected,
    #[error("camera session timed out")]
    Timeout,
    #[error("camera session cancelled")]
    Cancelled,
    #[error("camera returned invalid frame data: {0}")]
    InvalidFrame(&'static str),
    #[error("camera worker stopped unexpectedly")]
    WorkerStopped,
    #[error("camera I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    Grey,
    Yuyv,
    Rgb24,
    Other(String),
}

impl PixelFormat {
    pub fn fourcc(&self) -> &str {
        match self {
            Self::Grey => "GREY",
            Self::Yuyv => "YUYV",
            Self::Rgb24 => "RGB3",
            Self::Other(v) => v,
        }
    }
    pub fn preview_supported(&self) -> bool {
        !matches!(self, Self::Other(_))
    }
    fn bytes_per_pixel(&self) -> Result<usize, CameraError> {
        match self {
            Self::Grey => Ok(1),
            Self::Yuyv => Ok(2),
            Self::Rgb24 => Ok(3),
            Self::Other(_) => Err(CameraError::UnsupportedFormat),
        }
    }
    #[cfg(target_os = "linux")]
    fn from_code(code: u32) -> Self {
        match &code.to_le_bytes() {
            b"GREY" => Self::Grey,
            b"YUYV" => Self::Yuyv,
            b"RGB3" => Self::Rgb24,
            bytes => Self::Other(String::from_utf8_lossy(bytes).into_owned()),
        }
    }
}

impl fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.fourcc())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    Discrete {
        width: u32,
        height: u32,
    },
    Stepwise {
        min_width: u32,
        max_width: u32,
        step_width: u32,
        min_height: u32,
        max_height: u32,
        step_height: u32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormatInfo {
    pub pixel_format: PixelFormat,
    pub description: String,
    /// Empty when the driver does not implement frame-size enumeration.
    pub resolutions: Vec<Resolution>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrHint {
    /// Device name advertises IR; this is an untrusted descriptive hint.
    NameSuggestsInfrared,
    /// GREY is often a monochrome sensor; it is not proof of infrared sensing.
    MonochromeFormat,
    Unknown,
}

/// Selection binding, not cryptographic proof of hardware identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub sysfs_path: PathBuf,
    pub device_number: u64,
    pub driver: String,
    pub card: String,
    pub bus_info: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub path: PathBuf,
    pub name: String,
    pub driver: String,
    pub bus_info: String,
    pub identity: DeviceIdentity,
    pub formats: Vec<FormatInfo>,
    pub ir_hint: IrHint,
    pub capture_supported: bool,
    /// Per-device errors remain visible rather than hiding inaccessible devices.
    pub diagnostic: Option<String>,
}

/// Read-only V4L2 control inventory; it never sets controls or enables an emitter.
#[derive(Clone, Debug, Serialize)]
pub struct ControlInfo {
    pub id: u32,
    pub name: String,
    pub control_type: u32,
    pub minimum: i32,
    pub maximum: i32,
    pub step: i32,
    pub default_value: i32,
    pub flags: u32,
    pub current_value: Option<i32>,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ControlReport {
    pub controls: Vec<ControlInfo>,
    pub enumeration_complete: bool,
    pub diagnostic: Option<String>,
}

pub fn controls(path: &std::path::Path) -> Result<ControlReport, CameraError> {
    #[cfg(target_os = "linux")]
    {
        linux::controls(path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(CameraError::UnsupportedPlatform)
    }
}

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub device: DeviceInfo,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    /// Absolute session limit, including reconnection; maximum 60 seconds.
    pub timeout: Duration,
    pub max_frames: u32,
    /// At most three explicit same-path, same-identity re-enumeration attempts.
    pub reconnect_attempts: u8,
}

impl CaptureConfig {
    pub fn preview(device: DeviceInfo) -> Result<Self, CameraError> {
        if !device.capture_supported {
            return Err(CameraError::InvalidRequest(
                "device is not a single-planar streaming camera",
            ));
        }
        let format = device
            .formats
            .iter()
            .find(|f| f.pixel_format.preview_supported())
            .ok_or(CameraError::UnsupportedFormat)?;
        let (width, height) = format
            .resolutions
            .iter()
            .filter_map(|r| match r {
                Resolution::Discrete { width, height } => Some((*width, *height)),
                _ => None,
            })
            .filter(|(w, h)| *w > 0 && *h > 0 && *w <= 1920 && *h <= 1080)
            .min_by_key(|(w, h)| w.abs_diff(640) + h.abs_diff(480))
            .unwrap_or((640, 480));
        let pixel_format = format.pixel_format.clone();
        Ok(Self {
            device,
            width,
            height,
            pixel_format,
            timeout: Duration::from_secs(10),
            max_frames: 300,
            reconnect_attempts: 0,
        })
    }
    fn validate(&self) -> Result<(), CameraError> {
        if self.width == 0 || self.height == 0 || self.width > 8192 || self.height > 8192 {
            return Err(CameraError::InvalidRequest("dimensions must be 1..=8192"));
        }
        if self.timeout.is_zero() || self.timeout > Duration::from_secs(60) {
            return Err(CameraError::InvalidRequest(
                "timeout must be positive and at most 60 seconds",
            ));
        }
        if self.max_frames == 0 || self.max_frames > 3600 {
            return Err(CameraError::InvalidRequest("frame count must be 1..=3600"));
        }
        if self.reconnect_attempts > 3 {
            return Err(CameraError::InvalidRequest(
                "at most three reconnect attempts are allowed",
            ));
        }
        if !self.pixel_format.preview_supported() {
            return Err(CameraError::UnsupportedFormat);
        }
        if !self.device.capture_supported {
            return Err(CameraError::InvalidRequest("device cannot capture"));
        }
        Ok(())
    }
}

/// Raw frame buffer is redacted from Debug and wiped on drop. `to_rgb` is for preview.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: PixelFormat,
    pub sequence: u32,
    pub data: Vec<u8>,
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixel_format", &self.pixel_format)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        self.data.zeroize();
    }
}

impl Frame {
    fn validate(&self) -> Result<(usize, usize), CameraError> {
        if self.width == 0 || self.height == 0 || self.width > 8192 || self.height > 8192 {
            return Err(CameraError::InvalidFrame("dimensions out of bounds"));
        }
        if self.pixel_format == PixelFormat::Yuyv && !self.width.is_multiple_of(2) {
            return Err(CameraError::InvalidFrame("YUYV width must be even"));
        }
        let row = self.width as usize * self.pixel_format.bytes_per_pixel()?;
        let stride = self.stride as usize;
        if stride < row {
            return Err(CameraError::InvalidFrame("stride is shorter than a row"));
        }
        let required = stride
            .checked_mul(self.height as usize - 1)
            .and_then(|n| n.checked_add(row))
            .ok_or(CameraError::InvalidFrame("size overflow"))?;
        if required > MAX_FRAME_BYTES
            || self.data.len() > MAX_FRAME_BYTES
            || self.data.len() < required
        {
            return Err(CameraError::InvalidFrame("buffer size out of bounds"));
        }
        let rgb_len = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(3))
            .filter(|n| *n <= MAX_FRAME_BYTES)
            .ok_or(CameraError::InvalidFrame("RGB buffer exceeds memory limit"))?;
        Ok((row, rgb_len))
    }
    /// GREY is copied into all RGB channels. YUYV preview uses BT.601 limited range;
    /// it is a display conversion, not calibrated biometric color processing.
    pub fn to_rgb(&self) -> Result<Vec<u8>, CameraError> {
        let (row_len, rgb_len) = self.validate()?;
        let mut rgb = Vec::with_capacity(rgb_len);
        for y in 0..self.height as usize {
            let row = &self.data[y * self.stride as usize..y * self.stride as usize + row_len];
            match self.pixel_format {
                PixelFormat::Grey => {
                    for &value in row {
                        rgb.extend_from_slice(&[value, value, value]);
                    }
                }
                PixelFormat::Rgb24 => rgb.extend_from_slice(row),
                PixelFormat::Yuyv => {
                    for p in row.as_chunks::<4>().0 {
                        rgb.extend_from_slice(&yuv(p[0], p[1], p[3]));
                        rgb.extend_from_slice(&yuv(p[2], p[1], p[3]));
                    }
                }
                PixelFormat::Other(_) => return Err(CameraError::UnsupportedFormat),
            }
        }
        Ok(rgb)
    }
}

fn yuv(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = (i32::from(y) - 16).max(0);
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    [
        ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8,
        ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8,
        ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8,
    ]
}

pub fn enumerate() -> Result<Vec<DeviceInfo>, CameraError> {
    #[cfg(target_os = "linux")]
    {
        linux::enumerate()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(CameraError::UnsupportedPlatform)
    }
}

/// Owned session. Frames queue has capacity two; slow consumers drop new frames.
/// Drop requests cancellation; call `stop().await` to wait for resource release.
pub struct CameraSession {
    frames: mpsc::Receiver<Frame>,
    completion: Option<oneshot::Receiver<Result<(), CameraError>>>,
    cancel: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl CameraSession {
    pub fn start(config: CaptureConfig) -> Result<Self, CameraError> {
        config.validate()?;
        #[cfg(target_os = "linux")]
        {
            Self::spawn(config, linux::LinuxCamera::open)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(CameraError::UnsupportedPlatform)
        }
    }
    #[cfg(any(target_os = "linux", test))]
    fn spawn<S, F>(config: CaptureConfig, mut open: F) -> Result<Self, CameraError>
    where
        S: CameraSource,
        F: FnMut(&CaptureConfig) -> Result<S, CameraError> + Send + 'static,
    {
        let (tx, frames) = mpsc::channel(2);
        let (done, completion) = oneshot::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = thread::Builder::new()
            .name("hello-camera".into())
            .spawn(move || {
                let result = run_capture(&config, &worker_cancel, &tx, &mut open);
                drop(tx);
                let _ = done.send(result);
            })?;
        Ok(Self {
            frames,
            completion: Some(completion),
            cancel,
            worker: Some(worker),
        })
    }
    pub async fn next_frame(&mut self) -> Result<Option<Frame>, CameraError> {
        if let Some(frame) = self.frames.recv().await {
            return Ok(Some(frame));
        }
        if let Some(completion) = self.completion.as_mut() {
            // Keep the receiver in the session while awaiting: a caller may
            // cancel this future with select! without losing the final error.
            let result = completion.await;
            self.completion = None;
            result.map_err(|_| CameraError::WorkerStopped)??;
        }
        Ok(None)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub async fn stop(mut self) -> Result<(), CameraError> {
        self.cancel();
        self.frames.close();
        if let Some(worker) = self.worker.take() {
            tokio::task::spawn_blocking(move || worker.join())
                .await
                .map_err(|_| CameraError::WorkerStopped)?
                .map_err(|_| CameraError::WorkerStopped)?;
        }
        if let Some(completion) = self.completion.take() {
            match completion.await.map_err(|_| CameraError::WorkerStopped)? {
                Ok(()) | Err(CameraError::Cancelled) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl Drop for CameraSession {
    fn drop(&mut self) {
        self.cancel();
        self.frames.close();
    }
}

/// Private backend seam. Simulated sources only exist in cfg(test).
#[cfg(any(target_os = "linux", test))]
trait CameraSource {
    fn next(&mut self, wait: Duration) -> Result<Option<Frame>, CameraError>;
}

#[cfg(any(target_os = "linux", test))]
fn run_capture<S: CameraSource>(
    config: &CaptureConfig,
    cancel: &AtomicBool,
    frames: &mpsc::Sender<Frame>,
    open: &mut impl FnMut(&CaptureConfig) -> Result<S, CameraError>,
) -> Result<(), CameraError> {
    let deadline = Instant::now() + config.timeout;
    let mut source = Some(open(config)?);
    let mut captured = 0;
    let mut retries = 0;
    loop {
        if cancel.load(Ordering::Acquire) || frames.is_closed() {
            return Err(CameraError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CameraError::Timeout);
        }
        if source.is_none() {
            if retries >= config.reconnect_attempts {
                return Err(CameraError::Disconnected);
            }
            retries += 1;
            thread::sleep(remaining.min(POLL_INTERVAL));
            if cancel.load(Ordering::Acquire) {
                return Err(CameraError::Cancelled);
            }
            match open(config) {
                Ok(camera) => source = Some(camera),
                Err(CameraError::Disconnected) => continue,
                Err(e) => return Err(e),
            }
            continue;
        }
        let Some(camera) = source.as_mut() else {
            return Err(CameraError::WorkerStopped);
        };
        match camera.next(remaining.min(POLL_INTERVAL)) {
            Ok(Some(frame)) => {
                frame.validate()?;
                captured += 1;
                match frames.try_send(frame) {
                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        return Err(CameraError::Cancelled);
                    }
                }
                if captured >= config.max_frames {
                    return Ok(());
                }
            }
            Ok(None) => {}
            Err(CameraError::Disconnected) => {
                source.take();
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
