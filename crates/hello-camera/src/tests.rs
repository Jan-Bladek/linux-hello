//! Simulated tests. These do not establish physical-camera or IR compatibility.
use super::*;
use std::{collections::VecDeque, sync::atomic::AtomicUsize};

#[tokio::test]
async fn cancelled_receive_does_not_discard_terminal_capture_error() {
    let (tx, frames) = mpsc::channel(2);
    let (done, completion) = oneshot::channel();
    drop(tx);
    let mut session = CameraSession {
        frames,
        completion: Some(completion),
        cancel: Arc::new(AtomicBool::new(false)),
        worker: None,
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(5), session.next_frame())
            .await
            .is_err()
    );
    done.send(Err(CameraError::Disconnected)).unwrap();
    assert!(matches!(
        session.next_frame().await,
        Err(CameraError::Disconnected)
    ));
    session.stop().await.unwrap();
}

fn frame(format: PixelFormat, width: u32, height: u32, stride: u32, data: Vec<u8>) -> Frame {
    Frame {
        width,
        height,
        stride,
        pixel_format: format,
        sequence: 1,
        data,
    }
}
fn grey() -> Frame {
    frame(PixelFormat::Grey, 2, 1, 2, vec![0, 255])
}
fn device() -> DeviceInfo {
    DeviceInfo {
        path: "/dev/video0".into(),
        name: "Simulated test camera".into(),
        driver: "test".into(),
        bus_info: "test".into(),
        identity: DeviceIdentity::default(),
        formats: vec![FormatInfo {
            pixel_format: PixelFormat::Grey,
            description: "test".into(),
            resolutions: vec![Resolution::Discrete {
                width: 640,
                height: 480,
            }],
        }],
        ir_hint: IrHint::Unknown,
        capture_supported: true,
        diagnostic: None,
    }
}
fn config() -> CaptureConfig {
    CaptureConfig {
        device: device(),
        width: 640,
        height: 480,
        pixel_format: PixelFormat::Grey,
        timeout: Duration::from_secs(1),
        max_frames: 1,
        reconnect_attempts: 0,
    }
}
struct SimulatedCamera {
    events: VecDeque<Result<Option<Frame>, CameraError>>,
    drops: Arc<AtomicUsize>,
}
impl CameraSource for SimulatedCamera {
    fn next(&mut self, wait: Duration) -> Result<Option<Frame>, CameraError> {
        if let Some(event) = self.events.pop_front() {
            event
        } else {
            thread::sleep(wait);
            Ok(None)
        }
    }
}
impl Drop for SimulatedCamera {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn grey_conversion_ignores_row_padding() {
    let frame = frame(PixelFormat::Grey, 2, 2, 3, vec![0, 128, 99, 255, 64]);
    assert_eq!(
        frame.to_rgb().unwrap(),
        vec![0, 0, 0, 128, 128, 128, 255, 255, 255, 64, 64, 64]
    );
}
#[test]
fn rgb_conversion_ignores_padding() {
    let frame = frame(PixelFormat::Rgb24, 1, 2, 4, vec![1, 2, 3, 99, 4, 5, 6]);
    assert_eq!(frame.to_rgb().unwrap(), vec![1, 2, 3, 4, 5, 6]);
}
#[test]
fn yuyv_black_and_white_convert_correctly() {
    let frame = frame(PixelFormat::Yuyv, 2, 1, 4, vec![16, 128, 235, 128]);
    assert_eq!(frame.to_rgb().unwrap(), vec![0, 0, 0, 255, 255, 255]);
}
#[test]
fn rejects_truncated_odd_width_and_huge_frames() {
    for frame in [
        frame(PixelFormat::Grey, 2, 2, 2, vec![0; 3]),
        frame(PixelFormat::Yuyv, 1, 1, 2, vec![0; 2]),
        frame(PixelFormat::Rgb24, u32::MAX, 1, 1, vec![0]),
        frame(PixelFormat::Grey, 2, 1, 1, vec![0; 2]),
        frame(PixelFormat::Grey, 2, 2, u32::MAX, vec![0; 2]),
    ] {
        assert!(frame.to_rgb().is_err());
    }
}
#[test]
fn unknown_pixel_format_is_never_interpreted_as_rgb() {
    assert!(matches!(
        frame(PixelFormat::Other("MJPG".into()), 1, 1, 1, vec![0]).to_rgb(),
        Err(CameraError::UnsupportedFormat)
    ));
}
#[test]
fn frame_debug_never_contains_sample_bytes() {
    let debug = format!("{:?}", frame(PixelFormat::Grey, 3, 1, 3, vec![13, 37, 99]));
    assert!(!debug.contains("13, 37, 99"));
    assert!(!debug.contains("data"));
}
#[test]
fn unbounded_requests_are_rejected() {
    let mut cfg = config();
    cfg.timeout = Duration::ZERO;
    assert!(cfg.validate().is_err());
    cfg = config();
    cfg.timeout = Duration::from_secs(61);
    assert!(cfg.validate().is_err());
    cfg = config();
    cfg.max_frames = 3601;
    assert!(cfg.validate().is_err());
    cfg = config();
    cfg.reconnect_attempts = 4;
    assert!(cfg.validate().is_err());
    cfg = config();
    cfg.width = 0;
    assert!(cfg.validate().is_err());
}
#[test]
fn preview_uses_advertised_dimensions_and_rejects_compressed_only_device() {
    assert_eq!(CaptureConfig::preview(device()).unwrap().width, 640);
    let mut dev = device();
    dev.formats[0].pixel_format = PixelFormat::Other("MJPG".into());
    assert!(matches!(
        CaptureConfig::preview(dev),
        Err(CameraError::UnsupportedFormat)
    ));
}
#[tokio::test]
async fn finite_capture_delivers_frame_then_releases_camera() {
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = drops.clone();
    let mut session = CameraSession::spawn(config(), move |_| {
        Ok(SimulatedCamera {
            events: VecDeque::from([Ok(Some(grey()))]),
            drops: drops.clone(),
        })
    })
    .unwrap();
    assert_eq!(
        session
            .next_frame()
            .await
            .unwrap()
            .unwrap()
            .to_rgb()
            .unwrap(),
        vec![0, 0, 0, 255, 255, 255]
    );
    assert!(session.next_frame().await.unwrap().is_none());
    session.stop().await.unwrap();
    assert_eq!(observed.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn timeout_is_propagated_and_releases_source() {
    let mut cfg = config();
    cfg.timeout = Duration::from_millis(20);
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = drops.clone();
    let mut session = CameraSession::spawn(cfg, move |_| {
        Ok(SimulatedCamera {
            events: VecDeque::new(),
            drops: drops.clone(),
        })
    })
    .unwrap();
    assert!(matches!(
        session.next_frame().await,
        Err(CameraError::Timeout)
    ));
    session.stop().await.unwrap();
    assert_eq!(observed.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn cancellation_waits_for_release_without_blocking_full_queue() {
    let mut cfg = config();
    cfg.max_frames = 1000;
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = drops.clone();
    let session = CameraSession::spawn(cfg, move |_| {
        Ok(SimulatedCamera {
            events: (0..1000).map(|_| Ok(Some(grey()))).collect(),
            drops: drops.clone(),
        })
    })
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), session.stop())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(observed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dropping_session_cancels_idle_camera() {
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = drops.clone();
    let session = CameraSession::spawn(config(), move |_| {
        Ok(SimulatedCamera {
            events: VecDeque::new(),
            drops: drops.clone(),
        })
    })
    .unwrap();
    drop(session);
    tokio::time::timeout(Duration::from_secs(3), async {
        while observed.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(observed.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn slow_consumer_has_two_frame_bound() {
    let mut cfg = config();
    cfg.max_frames = 40;
    let session = CameraSession::spawn(cfg, move |_| {
        Ok(SimulatedCamera {
            events: (0..40).map(|_| Ok(Some(grey()))).collect(),
            drops: Arc::new(AtomicUsize::new(0)),
        })
    })
    .unwrap();
    // Join proves the producer cannot deadlock when the queue is full.
    let mut session = session;
    let worker = session.worker.take().unwrap();
    tokio::task::spawn_blocking(move || worker.join())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.frames.len(), 2);
    session.stop().await.unwrap();
}
#[tokio::test]
async fn malformed_backend_frame_never_reaches_consumer() {
    let mut session = CameraSession::spawn(config(), move |_| {
        Ok(SimulatedCamera {
            events: VecDeque::from([Ok(Some(frame(PixelFormat::Grey, 10, 10, 10, vec![0])))]),
            drops: Arc::new(AtomicUsize::new(0)),
        })
    })
    .unwrap();
    assert!(matches!(
        session.next_frame().await,
        Err(CameraError::InvalidFrame(_))
    ));
    session.stop().await.unwrap();
}
#[tokio::test]
async fn reconnect_releases_old_source_and_obeys_limit() {
    let mut cfg = config();
    cfg.reconnect_attempts = 2;
    let opens = Arc::new(AtomicUsize::new(0));
    let observed = opens.clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let observed_drops = drops.clone();
    let mut session = CameraSession::spawn(cfg, move |_| {
        opens.fetch_add(1, Ordering::SeqCst);
        Ok(SimulatedCamera {
            events: VecDeque::from([Err(CameraError::Disconnected)]),
            drops: drops.clone(),
        })
    })
    .unwrap();
    assert!(matches!(
        session.next_frame().await,
        Err(CameraError::Disconnected)
    ));
    session.stop().await.unwrap();
    assert_eq!(observed.load(Ordering::SeqCst), 3);
    assert_eq!(observed_drops.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn reconnect_can_recover_same_device() {
    let mut cfg = config();
    cfg.reconnect_attempts = 1;
    let opens = Arc::new(AtomicUsize::new(0));
    let mut session = CameraSession::spawn(cfg, move |_| {
        let attempt = opens.fetch_add(1, Ordering::SeqCst);
        Ok(SimulatedCamera {
            events: VecDeque::from([if attempt == 0 {
                Err(CameraError::Disconnected)
            } else {
                Ok(Some(grey()))
            }]),
            drops: Arc::new(AtomicUsize::new(0)),
        })
    })
    .unwrap();
    assert!(session.next_frame().await.unwrap().is_some());
    assert!(session.next_frame().await.unwrap().is_none());
    session.stop().await.unwrap();
}
#[tokio::test]
async fn substituted_device_aborts_reconnect() {
    let mut cfg = config();
    cfg.reconnect_attempts = 3;
    let opens = Arc::new(AtomicUsize::new(0));
    let observed = opens.clone();
    let mut session = CameraSession::spawn(cfg, move |_| {
        if opens.fetch_add(1, Ordering::SeqCst) > 0 {
            return Err(CameraError::DeviceChanged);
        }
        Ok(SimulatedCamera {
            events: VecDeque::from([Err(CameraError::Disconnected)]),
            drops: Arc::new(AtomicUsize::new(0)),
        })
    })
    .unwrap();
    assert!(matches!(
        session.next_frame().await,
        Err(CameraError::DeviceChanged)
    ));
    session.stop().await.unwrap();
    assert_eq!(observed.load(Ordering::SeqCst), 2);
}
