#![deny(unsafe_code)]

use hello_camera::{CameraSession, CaptureConfig, DeviceInfo};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

// Slint's generated vtables contain unsafe code; application logic stays safe.
#[allow(unsafe_code)]
mod generated_ui {
    slint::include_modules!();
}
use generated_ui::SettingsWindow;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let window = SettingsWindow::new()?;
    let devices: Arc<Mutex<Vec<DeviceInfo>>> = Arc::default();
    let cancel = Arc::new(AtomicBool::new(false));

    let weak = window.as_weak();
    let device_state = devices.clone();
    let handle = runtime.handle().clone();
    window.on_refresh(move || {
        let Some(ui) = weak.upgrade() else { return; };
        if ui.get_refreshing() || ui.get_preview_running() { return; }
        ui.set_refreshing(true);
        let session = ui.get_session_bus();
        let weak = weak.clone();
        let state = device_state.clone();
        handle.spawn(async move {
            let camera = tokio::task::spawn_blocking(hello_camera::enumerate).await;
            let daemon = hello_ipc::status(session).await;
            let fingerprints = hello_fingerprint::readers().await;
            let (found, camera_message, camera_json) = match camera {
                Ok(Ok(found)) => {
                    let message = if found.is_empty() { "No accessible V4L2 devices found".into() } else { format!("{} V4L2 device(s) found. Camera is off.", found.len()) };
                    let json = serde_json::to_string_pretty(&found).unwrap_or_else(|_| "Device encoding failed".into());
                    (found, message, json)
                }
                Ok(Err(error)) => (Vec::new(), error.to_string(), "Camera diagnostics unavailable".into()),
                Err(_) => (Vec::new(), "Camera worker failed".into(), "Camera diagnostics unavailable".into()),
            };
            let labels: Vec<String> = found.iter().map(|d| format!("{} · {}", d.path.display(), d.name)).collect();
            if let Ok(mut state) = state.lock() { *state = found; }
            let daemon_message = match daemon {
                Ok(status) => format!("Connected to {} · diagnostics only{}", status.capabilities.service_version, if session { " (session bus)" } else { "" }),
                Err(error) => error.to_string(),
            };
            let fingerprint_json = match fingerprints {
                Ok(readers) => serde_json::to_string_pretty(&readers).unwrap_or_else(|_| "Encoding failed".into()),
                Err(error) => error.to_string(),
            };
            let diagnostics = format!("Linux Hello {}\n\nDAEMON\n{}\n\nCAMERAS\n{}\n\nFPRINTD\n{}\n\nFace authentication and PAM are unavailable.", env!("CARGO_PKG_VERSION"), daemon_message, camera_json, fingerprint_json);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_devices(ModelRc::from(Rc::new(VecModel::from(labels.into_iter().map(SharedString::from).collect::<Vec<_>>()))));
                ui.set_selected_device(0);
                ui.set_camera_message(camera_message.into());
                ui.set_daemon_message(daemon_message.into());
                ui.set_diagnostics(diagnostics.into());
                ui.set_refreshing(false);
            });
        });
    });

    let weak = window.as_weak();
    let device_state = devices.clone();
    let stop = cancel.clone();
    let handle = runtime.handle().clone();
    window.on_start_preview(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_preview_running() || ui.get_refreshing() {
            return;
        }
        let device = device_state.lock().ok().and_then(|list| {
            usize::try_from(ui.get_selected_device())
                .ok()
                .and_then(|index| list.get(index).cloned())
        });
        let Some(device) = device else {
            return;
        };
        let config = CaptureConfig::preview(device);
        let mut config = match config {
            Ok(config) => config,
            Err(error) => {
                ui.set_preview_message(error.to_string().into());
                return;
            }
        };
        config.timeout = Duration::from_secs(ui.get_timeout_seconds().clamp(1, 30) as u64);
        config.max_frames = 300;
        config.reconnect_attempts = 0;
        stop.store(false, Ordering::Release);
        ui.set_preview_running(true);
        ui.set_preview_message("Opening camera…".into());
        let weak = weak.clone();
        let stop = stop.clone();
        handle.spawn(async move {
            let result = run_preview(config, weak.clone(), stop).await;
            let message = match result {
                Ok(()) => "Preview ended. Camera released; no frames saved.".into(),
                Err(error) => format!("Preview stopped: {error}"),
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_preview_running(false);
                ui.set_preview_image(slint::Image::default());
                ui.set_preview_message(message.into());
            });
        });
    });
    let stop = cancel.clone();
    window.on_stop_preview(move || stop.store(true, Ordering::Release));
    let result = window.run();
    cancel.store(true, Ordering::Release);
    // The capture worker has a bounded polling interval and observes cancellation.
    runtime.shutdown_timeout(Duration::from_secs(2));
    result?;
    Ok(())
}

async fn run_preview(
    config: CaptureConfig,
    weak: slint::Weak<SettingsWindow>,
    cancel: Arc<AtomicBool>,
) -> Result<(), hello_camera::CameraError> {
    let mut camera = CameraSession::start(config)?;
    // Bound UI queue to a single frame if the window is slow to draw.
    let pending = Arc::new(AtomicBool::new(false));
    let result = async {
        loop {
            if cancel.load(Ordering::Acquire) {
                camera.cancel();
                break;
            }
            let frame = tokio::select! {
                frame = camera.next_frame() => frame?,
                _ = tokio::time::sleep(Duration::from_millis(50)) => continue,
            };
            let Some(frame) = frame else {
                break;
            };
            if pending.swap(true, Ordering::AcqRel) {
                continue;
            }
            let pixels = frame.to_rgb()?;
            let (width, height) = (frame.width, frame.height);
            let pending = pending.clone();
            if weak
                .upgrade_in_event_loop(move |ui| {
                    let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
                        &pixels, width, height,
                    );
                    ui.set_preview_image(slint::Image::from_rgb8(buffer));
                    ui.set_preview_message(
                        format!("Live preview · {width} × {height} · no frames saved").into(),
                    );
                    pending.store(false, Ordering::Release);
                })
                .is_err()
            {
                camera.cancel();
                break;
            }
        }
        Ok(())
    }
    .await;
    let stopped = camera.stop().await;
    result.and(stopped)
}

#[cfg(test)]
mod rendering_tests {
    use super::*;
    use slint::platform::{
        Platform, PlatformError, WindowAdapter,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    };
    use std::io::Write;

    struct Headless(Rc<MinimalSoftwareWindow>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.0.clone())
        }
    }

    /// Exercise the actual compiled Slint layout/renderer at two window sizes,
    /// including every tab. Optional snapshots contain UI only; no camera opens.
    #[test]
    fn render_all_pages_at_desktop_and_minimum_size() {
        let backend = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(Headless(backend.clone()))).unwrap();
        let ui = SettingsWindow::new().unwrap();
        ui.show().unwrap();
        for (width, height) in [(980, 740), (760, 660)] {
            backend.set_size(slint::PhysicalSize::new(width, height));
            for page in 0..5 {
                ui.set_page(page);
                let pixels = ui.window().take_snapshot().unwrap();
                assert_eq!(pixels.width(), width);
                assert_eq!(pixels.height(), height);
                assert!(
                    pixels.as_slice().windows(2).any(|pair| pair[0] != pair[1]),
                    "UI must contain rendered content"
                );
                if let Some(directory) = std::env::var_os("LINUX_HELLO_UI_SNAPSHOT_DIR") {
                    std::fs::create_dir_all(&directory).unwrap();
                    let path =
                        std::path::Path::new(&directory).join(format!("page-{page}-{width}.ppm"));
                    let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
                    write!(file, "P6\n{width} {height}\n255\n").unwrap();
                    for pixel in pixels.as_slice() {
                        file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
                    }
                }
            }
        }
        ui.hide().unwrap();
    }
}
