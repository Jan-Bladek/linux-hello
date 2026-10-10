//! Explicitly opt-in, non-privileged test. Never captures during ordinary CI.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires an explicitly selected physical camera and a local user"]
async fn selected_camera_captures_releases_and_reopens() {
    use hello_camera::{CameraSession, CaptureConfig, enumerate};
    use std::time::Duration;
    let path = std::env::var_os("LINUX_HELLO_CAMERA")
        .expect("set LINUX_HELLO_CAMERA to the selected /dev/videoN node");
    let devices = enumerate().expect("enumerate camera devices");
    let device = devices
        .into_iter()
        .find(|d| d.path.as_os_str() == path)
        .expect("selected device must be present");
    assert!(
        device.capture_supported,
        "device is not a single-planar streaming capture camera: {:?}",
        device.diagnostic
    );
    for _ in 0..2 {
        let mut config = CaptureConfig::preview(device.clone())
            .expect("camera needs GREY, YUYV, or RGB3 preview support");
        config.max_frames = 5;
        config.timeout = Duration::from_secs(10);
        let mut session = CameraSession::start(config).expect("start camera worker");
        let mut frames = 0;
        while let Some(frame) = session.next_frame().await.expect("capture a valid frame") {
            let rgb = frame.to_rgb().expect("convert negotiated frame layout");
            assert_eq!(rgb.len(), frame.width as usize * frame.height as usize * 3);
            frames += 1;
        }
        assert!(frames > 0, "camera returned no usable frames");
        session.stop().await.expect("release camera resources");
    }
    // This proves only capture and reopen in this environment, not IR spectrum,
    // illumination, ToF access, face recognition, presentation resistance or PAM.
}
