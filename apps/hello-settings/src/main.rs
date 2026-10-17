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
#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
mod system_setup;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
    hello_cli::process_io::protect_secret_process()?;
    let runtime = tokio::runtime::Runtime::new()?;
    let window = SettingsWindow::new()?;
    if let Ok(output) = std::process::Command::new("/usr/bin/id")
        .arg("-un")
        .output()
        && output.status.success()
    {
        window.set_account_name(String::from_utf8_lossy(&output.stdout).trim().into());
    }
    let devices: Arc<Mutex<Vec<DeviceInfo>>> = Arc::default();
    let cancel = Arc::new(AtomicBool::new(false));

    let active_job = Arc::new(Mutex::new(None::<tokio::task::JoinHandle<()>>));
    #[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
    configure_face_check(&window, &runtime, &devices, &cancel, &active_job);

    let weak = window.as_weak();
    let device_state = devices.clone();
    let handle = runtime.handle().clone();
    window.on_refresh(move || {
        let Some(ui) = weak.upgrade() else { return; };
        if ui.get_refreshing() || ui.get_preview_running() || ui.get_face_check_running() { return; }
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
                    let message = if found.is_empty() { "No accessible camera found. Check its connection and session permissions.".into() } else { format!("Available cameras: {}. The camera stays off until you start a scan or preview.", found.len()) };
                    let json = serde_json::to_string_pretty(&found).unwrap_or_else(|_| "Device encoding failed".into());
                    (found, message, json)
                }
                Ok(Err(error)) => (Vec::new(), error.to_string(), "Camera diagnostics unavailable".into()),
                Err(_) => (Vec::new(), "Camera worker failed".into(), "Camera diagnostics unavailable".into()),
            };
            let labels: Vec<String> = found.iter().map(|d| format!("{} · {}", if d.ir_hint != hello_camera::IrHint::Unknown { "IR camera" } else { "Camera" }, d.name)).collect();
            let selected = found.iter().position(|d| d.capture_supported && d.ir_hint != hello_camera::IrHint::Unknown).unwrap_or(0) as i32;
            let ir_preferred = found.get(selected as usize).is_some_and(|d| d.ir_hint != hello_camera::IrHint::Unknown);
            if let Ok(mut state) = state.lock() { *state = found; }
            let daemon_message = match daemon {
                Ok(status) => format!("Connected to {} · diagnostics only{}", status.capabilities.service_version, if session { " (session bus)" } else { "" }),
                Err(error) => error.to_string(),
            };
            let fingerprint_json = match fingerprints {
                Ok(readers) => serde_json::to_string_pretty(&readers).unwrap_or_else(|_| "Encoding failed".into()),
                Err(error) => error.to_string(),
            };
            let diagnostics = format!("Linux Hello {}\n\nDAEMON\n{}\n\nCAMERAS\n{}\n\nFPRINTD\n{}\n\nD-Bus is diagnostic. System authentication is configured separately with administrator authorization.", env!("CARGO_PKG_VERSION"), daemon_message, camera_json, fingerprint_json);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_devices(ModelRc::from(Rc::new(VecModel::from(labels.into_iter().map(SharedString::from).collect::<Vec<_>>()))));
                ui.set_selected_device(selected);
                // Selecting IR is only a preference. The scoped lease still
                // validates exact hardware/firmware before any control write.
                ui.set_use_ir_emitter(ir_preferred);
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
    let job_state = active_job.clone();
    window.on_start_preview(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_preview_running() || ui.get_face_check_running() || ui.get_refreshing() {
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
        let ir_emitter = ui.get_use_ir_emitter();
        stop.store(false, Ordering::Release);
        ui.set_preview_running(true);
        ui.set_preview_message("Starting camera…".into());
        let weak = weak.clone();
        let stop = stop.clone();
        let job = handle.spawn(async move {
            let result = run_preview(config, weak.clone(), stop, ir_emitter).await;
            let message = match result {
                Ok(()) if ir_emitter => {
                    "Preview finished. Camera released and original IR mode restored; no images were saved."
                        .into()
                }
                Ok(()) => "Preview finished. Camera is off; no images were saved.".into(),
                Err(error) => format!("Preview stopped: {error}"),
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_preview_running(false);
                ui.set_preview_image(slint::Image::default());
                ui.set_preview_message(message.into());
            });
        });
        *job_state.lock().expect("active camera job mutex") = Some(job);
    });
    let stop = cancel.clone();
    window.on_stop_preview(move || stop.store(true, Ordering::Release));
    // Discover devices on launch without opening the camera. Prefer the IR node
    // during discovery; capture itself always needs an explicit action.
    window.invoke_refresh();
    let result = window.run();
    cancel.store(true, Ordering::Release);
    // Await camera stop, emitter restoration and worker reaping before runtime teardown.
    if let Some(job) = active_job.lock().expect("active camera job mutex").take() {
        runtime.block_on(async {
            let _ = job.await;
        });
    }
    // Remaining refresh jobs do not access the camera stream.
    runtime.shutdown_timeout(Duration::from_secs(2));
    result?;
    Ok(())
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn configure_account_controls(
    window: &SettingsWindow,
    runtime: &tokio::runtime::Runtime,
    active_job: &Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
) {
    window.set_system_controls_available(
        std::path::Path::new("/usr/local/libexec/linux-hello-sudo/manage-person.py").is_file(),
    );
    window.set_health_check_available(true);
    for action in ["status", "pause", "remove", "health"] {
        let weak = window.as_weak();
        let jobs = active_job.clone();
        let handle = runtime.handle().clone();
        let callback = move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if ui.get_face_check_running()
                || ui.get_preview_running()
                || ui.get_refreshing()
                || (action == "remove" && !ui.get_confirm_system_removal())
            {
                return;
            }
            ui.set_face_check_running(true);
            ui.set_reference_io_running(true);
            ui.set_confirm_system_removal(false);
            ui.set_account_status(
                if action == "health" {
                    "Checking installation health…"
                } else {
                    "Authorize this account operation in the system dialog…"
                }
                .into(),
            );
            let weak = weak.clone();
            let job = handle.spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    if action == "health" { Ok((hello_cli::health::inspect().text(), None)) }
                    else { hello_cli::account::manage(action).map(|status| (status.summary(), Some(status))) }
                }).await;
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_face_check_running(false);
                    ui.set_reference_io_running(false);
                    match result {
                        Ok(Ok((message, status))) => {
                            if let Some(status) = status {
                                ui.set_system_configured(status.sudo || status.login || status.unlock);
                                ui.set_account_status(message.into());
                            } else {
                                ui.set_diagnostics(message.into());
                                ui.set_account_status("Installation health report ready in Help. This is not an authentication result.".into());
                                ui.set_page(4);
                            }
                        }
                        Ok(Err(error)) => ui.set_account_status(error.into()),
                        Err(_) => ui.set_account_status("Account operation did not finish. Refresh status; password fallback remains available.".into()),
                    }
                });
            });
            *jobs.lock().expect("active account job mutex") = Some(job);
        };
        match action {
            "status" => window.on_refresh_account_status(callback),
            "pause" => window.on_pause_account(callback),
            "remove" => window.on_remove_account(callback),
            _ => window.on_run_health_check(callback),
        }
    }
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum FaceAction {
    ScanPerson,
    Check,
    CompareThree,
    Recognize,
    MotionDiagnostic,
    TestSavedPerson,
    TestOtherPerson,
    Remember,
    AddSample,
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
impl FaceAction {
    fn trial_label(self) -> Option<hello_cli::diagnostics::TrialLabel> {
        use hello_cli::diagnostics::TrialLabel;
        match self {
            Self::TestSavedPerson => Some(TrialLabel::SavedPerson),
            Self::TestOtherPerson => Some(TrialLabel::OtherPerson),
            _ => None,
        }
    }
    fn is_recognition(self) -> bool {
        self == Self::Recognize || self == Self::MotionDiagnostic || self.trial_label().is_some()
    }
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn configure_face_check(
    window: &SettingsWindow,
    runtime: &tokio::runtime::Runtime,
    devices: &Arc<Mutex<Vec<DeviceInfo>>>,
    cancel: &Arc<AtomicBool>,
    active_job: &Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
) {
    window.set_face_check_available(true);
    window.set_recognition_threshold(
        hello_cli::diagnostics::DEFAULT_RECOGNITION_THRESHOLD
            .to_string()
            .into(),
    );
    window.set_sample_target(hello_cli::diagnostics::REFERENCE_SAMPLES as i32);
    window.set_face_check_message(
        "Choose a camera and select Scan a new face. Name it after scanning.".into(),
    );
    let models = std::env::var_os("LINUX_HELLO_MODELS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let installed = std::path::PathBuf::from("/usr/local/libexec/linux-hello-sudo/models");
            if installed.is_dir() {
                installed
            } else {
                "work/inference/models".into()
            }
        });
    if let Ok(models) = models.canonicalize() {
        window.set_models_directory(models.to_string_lossy().as_ref().into());
    } else {
        window.set_face_check_message("Recognition models are missing. Install the full application or select a model folder in developer mode.".into());
    }
    let reference = Arc::new(Mutex::new(
        hello_cli::diagnostics::SessionReference::default(),
    ));
    configure_reference_storage(window, runtime, &reference, active_job);
    configure_account_controls(window, runtime, active_job);
    let person_reference = reference.clone();
    let person_devices = devices.clone();
    let trials = Arc::new(Mutex::new(
        hello_cli::diagnostics::RecognitionTrials::default(),
    ));
    let clear_trials = trials.clone();
    let clear_ui = window.as_weak();
    window.on_clear_trials(move || {
        if let Some(ui) = clear_ui.upgrade() {
            if ui.get_face_check_running() {
                return;
            }
            let mut trials = clear_trials.lock().expect("recognition trials mutex");
            trials.clear();
            ui.set_trial_message(trials.message().into());
        }
    });
    let forget_reference = reference.clone();
    let forget_ui = window.as_weak();
    window.on_forget_face(move || {
        let Some(ui) = forget_ui.upgrade() else {
            return;
        };
        if ui.get_face_check_running() {
            return;
        }
        let mut reference = forget_reference.lock().expect("session reference mutex");
        reference.clear();
        sync_person_ui(&ui, &reference);
        ui.set_recognition_state(0);
        ui.set_face_check_message(
            "Memory reference cleared. Use Delete saved in Settings to remove a saved file.".into(),
        );
    });
    let weak = window.as_weak();
    let devices = devices.clone();
    let cancel = cancel.clone();
    let active_job = active_job.clone();
    let handle = runtime.handle().clone();
    let capture = Rc::new(move |action: FaceAction| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_preview_running() || ui.get_face_check_running() || ui.get_refreshing() {
            return;
        }
        let device = devices.lock().ok().and_then(|list| {
            usize::try_from(ui.get_selected_device())
                .ok()
                .and_then(|i| list.get(i).cloned())
        });
        let Some(device) = device else {
            return;
        };
        let executable = match std::env::current_exe() {
            Ok(path) => path.with_file_name("hello-face-worker"),
            Err(error) => {
                ui.set_face_check_message(error.to_string().into());
                return;
            }
        };
        let models = std::path::PathBuf::from(ui.get_models_directory().as_str());
        let ir = ui.get_use_ir_emitter();
        let threshold = if action.is_recognition() {
            if ui.get_person_needs_name() {
                ui.set_face_check_message("Name the scanned face first.".into());
                return;
            }
            let result = parse_recognition_threshold(ui.get_recognition_threshold().as_str())
                .and_then(|threshold| {
                    let reference = reference.lock().map_err(|_| "reference unavailable")?;
                    reference.validate_strict_recognition(&device.identity, ir)?;
                    if action.trial_label().is_some() {
                        trials
                            .lock()
                            .map_err(|_| "trials unavailable")?
                            .validate(&reference, threshold)?;
                    }
                    Ok(threshold)
                });
            match result {
                Ok(threshold) => Some(threshold),
                Err(error) => {
                    ui.set_recognition_state(4);
                    ui.set_recognition_title("Cannot start recognition".into());
                    ui.set_face_check_message(format!("Recognition unavailable: {error}").into());
                    if action.trial_label().is_some() {
                        ui.set_trial_message(format!("Trial unavailable: {error}").into());
                    }
                    return;
                }
            }
        } else {
            None
        };
        cancel.store(false, Ordering::Release);
        ui.set_scan_frame_available(false);
        ui.set_preview_image(slint::Image::default());
        ui.set_face_check_running(true);
        ui.set_face_check_message(if action.is_recognition() {
            "Recognizing… Look at the camera and hold still. Stop cancels this attempt.".into()
        } else if action == FaceAction::CompareThree {
            "Comparing 3 fresh samples… Look at the camera and hold still. Stop cancels the series."
                .into()
        } else if action != FaceAction::Check {
            "Capturing a sample… Hold still. Stop cancels the current sample.".into()
        } else {
            "Checking face detection… Stop cancels the check.".into()
        });
        if action.trial_label().is_some() {
            ui.set_trial_message(
                "Trial running… Stop cancels; closing the window also cancels.".into(),
            );
        }
        if action.is_recognition() {
            ui.set_recognition_state(5);
            ui.set_recognition_title("Checking your face…".into());
        }
        let cancel = cancel.clone();
        let weak = weak.clone();
        let reference = reference.clone();
        let trials = trials.clone();
        let (observer, display_alive) = scan_preview_observer(weak.clone(), cancel.clone());
        let job = handle.spawn(async move {
            let live = if action == FaceAction::ScanPerson || action == FaceAction::CompareThree || action.is_recognition() {
                match hello_cli::diagnostics::LiveCapture::open(executable.clone(),device.path.clone(),models.clone(),ir,device.identity.clone(),cancel.clone(),Some(observer.clone())).await {
                    Ok(mut capture) => {
                        let status_ui=weak.clone();let alive=display_alive.clone();
                        capture.set_status_observer(Arc::new(move |message| {
                            let alive=alive.clone();
                            let _=status_ui.upgrade_in_event_loop(move |ui| {
                                if alive.load(Ordering::Acquire) && ui.get_face_check_running() {
                                    ui.set_face_check_message(message.clone().into());
                                }
                            });
                        }));
                        Some(Arc::new(tokio::sync::Mutex::new(capture)))
                    }
                    Err(error) => {
                        let diagnostic=serde_json::json!({"development_diagnostic":true,"selected_device":device.path,
                            "active_threshold":threshold.map(|t|t.value()),"recognition_outcome":"acquisition_failed",
                            "failure_reason":error.to_string(),"authentication_ready":false,"raw_frames_saved":false,"embeddings_saved":false});
                        display_alive.store(false,Ordering::Release);
                        let _=weak.upgrade_in_event_loop(move |ui| {
                            ui.set_face_check_running(false);ui.set_scan_frame_available(false);ui.set_preview_image(slint::Image::default());
                            ui.set_recognition_state(4);ui.set_recognition_title("Cannot start scanning".into());
                            ui.set_face_check_message(format!("{error}").into());
                            ui.set_diagnostics(serde_json::to_string_pretty(&diagnostic).unwrap().into());
                        });
                        return;
                    }
                }
            } else { None };
            if action == FaceAction::ScanPerson {
                let progress_ui = weak.clone();
                let mut previous_count = None;
                let feedback_ui = weak.clone();
                let (expected_revision, staged) = reference.lock().expect("session reference mutex").stage_enrollment();
                let staged = std::sync::Mutex::new(staged);
                let result = hello_cli::diagnostics::collect_reference_observed(&staged, &device.identity, ir, &cancel,
                    |count| {
                        let changed = previous_count != Some(count); previous_count = Some(count);
                        let capture=live.as_ref().expect("live scan session").clone(); let cancel=cancel.clone();
                        async move {
                            {
                                for _ in 0..if changed {30} else {8} {
                                    if cancel.load(Ordering::Acquire) { return Err("face scan cancelled".into()); }
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                }
                            }
                            capture.lock().await.next_sample().await
                        }
                    },
                    move |count| { let _ = progress_ui.upgrade_in_event_loop(move |ui| { ui.set_sample_count(count as i32); ui.set_sample_instruction(person_instruction(count).into()); }); },
                    move |hint| { let hint=hint.to_owned(); let _=feedback_ui.upgrade_in_event_loop(move |ui| {if ui.get_face_check_running() {ui.set_face_check_message(hint.into());}}); },
                ).await;
                let mut capture=live.as_ref().expect("live scan session").lock().await;
                let cleanup=capture.close().await;let capture_report=capture.report();drop(capture);
                let success = cleanup.is_ok() && result.is_ok() && !cancel.load(Ordering::Acquire);
                let commit = reference.lock().expect("session reference mutex").finish_enrollment(
                    expected_revision, staged.into_inner().expect("staged reference mutex"), success);
                let result=cleanup.and(result).and(commit);
                let result = if cancel.load(Ordering::Acquire) { Err("Scan cancelled".into()) } else { result };
                display_alive.store(false, Ordering::Release);
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_face_check_running(false); ui.set_scan_frame_available(false); ui.set_preview_image(slint::Image::default());
                    sync_person_ui(&ui, &reference.lock().expect("session reference mutex"));
                    ui.set_diagnostics(serde_json::to_string_pretty(&capture_report).unwrap().into());
                    ui.set_face_check_message(match result {
                        Ok(_) => "Face scanned. Give it a name, then scan again to verify it.".into(),
                        Err(error) => format!("Scan stopped: {error}. The previous face was retained. Incomplete samples can be resumed; a completed failed scan must be repeated.").into(),
                    });
                });
                return;
            }
            if action == FaceAction::CompareThree || action.is_recognition() {
                let result = if action == FaceAction::MotionDiagnostic {
                    let progress_ui = weak.clone();
                    let feedback_ui = weak.clone();
                    let mut previous_step = None;
                    hello_cli::diagnostics::recognize_person_observed(&reference, &device.identity, ir, &cancel,
                        |step| {
                            let changed = previous_step != Some(step); previous_step = Some(step);
                            let weak=progress_ui.clone();let capture=live.as_ref().expect("live recognition session").clone();let cancel=cancel.clone();
                            async move {
                                if changed { let _ = weak.upgrade_in_event_loop(move |ui| { ui.set_recognition_title(step.instruction().into()); ui.set_face_check_message(if step == hello_cli::diagnostics::MotionStep::Front { "Look straight ahead and hold still. Establishing your natural position; no movement is needed yet." } else { "Only a small turn is needed. Keep both eyes visible and hold still." }.into()); }); }
                                // Give the person time to perform the measured pose, polling Stop.
                                for _ in 0..if changed {30} else {8} { if cancel.load(Ordering::Acquire) { return Err("Recognition cancelled".into()); } tokio::time::sleep(Duration::from_millis(100)).await; }
                                capture.lock().await.next_sample().await
                            }
                        }, move |feedback| {
                            let _ = feedback_ui.upgrade_in_event_loop(move |ui| {
                                ui.set_recognition_title(motion_progress_title(&feedback).into());
                                ui.set_face_check_message(feedback.hint.into());
                            });
                        }).await
                } else if action.is_recognition() {
                    let feedback_ui=weak.clone(); let mut first=true;
                    hello_cli::diagnostics::verify_person_observed(&reference,&device.identity,ir,&cancel,
                        || {let capture=live.as_ref().expect("live passive verification").clone();let cancel=cancel.clone();
                            let wait=if first {10} else {3};first=false;
                            async move {
                                for _ in 0..wait {if cancel.load(Ordering::Acquire) {return Err("Recognition cancelled".into());} tokio::time::sleep(Duration::from_millis(100)).await;}
                                capture.lock().await.next_sample().await
                            }
                        }, move |feedback| {let _=feedback_ui.upgrade_in_event_loop(move |ui| {
                            if ui.get_face_check_running() && ui.get_recognition_state()==5 {
                                ui.set_recognition_title(format!("Look straight at the camera · {}/3",feedback.accepted).into());
                                ui.set_face_check_message(feedback.hint.into());
                            }
                        });}).await
                } else {
                    hello_cli::diagnostics::compare_three(&reference, &device.identity, ir, &cancel,
                        || {let capture=live.as_ref().expect("live comparison session").clone();async move {capture.lock().await.next_sample().await}}).await
                };
                let mut capture=live.as_ref().expect("live recognition session").lock().await;
                let cleanup=capture.close().await;let capture_report=capture.report();drop(capture);
                let result=cleanup.and(result);
                display_alive.store(false, Ordering::Release);
                // Commit a trial on the UI thread so a queued Stop cannot be
                // counted as a completed trial before its result is displayed.
                let _ = weak.upgrade_in_event_loop(move |ui| {
                let mut trial_message = None;
                let mut trial_report = None;
                if let (Some(label), Some(threshold)) = (action.trial_label(), threshold) {
                    let reference = reference.lock().expect("session reference mutex");
                    let mut trials = trials.lock().expect("recognition trials mutex");
                    {
                        use hello_cli::diagnostics::TrialOutcome;
                        let outcome = if cancel.load(Ordering::Acquire) { TrialOutcome::Cancelled }
                            else { result.as_ref().map(|summary| TrialOutcome::Decision(summary.classify(threshold))).unwrap_or(TrialOutcome::AcquisitionFailed) };
                        if let Err(error) = trials.record(&reference, threshold, label, outcome) {
                            trial_message = Some(format!("Trial not recorded: {error}"));
                        }
                    }
                    if trial_message.is_none() { trial_message = Some(trials.message()); }
                    trial_report = Some(trials.report());
                }
                let (message, report) = match result {
                    Ok(_) if cancel.load(Ordering::Acquire) => ("Comparison cancelled. No complete result.".into(), String::new()),
                    Ok(summary) => {
                        let mut report = serde_json::to_value(&summary).expect("finite comparison summary");
                        report["capture"]=capture_report.clone();
                        if let Some(trials) = &trial_report { report["manual_trials"] = trials.clone(); }
                        let message = if let Some(threshold) = threshold {
                            let decision = summary.classify(threshold);
                            report["recognition_decision"] = serde_json::to_value(decision).unwrap();
                            report["recognition_threshold"] = threshold.value().into();
                            if report["failure_reason"].is_null() {
                                report["failure_reason"] = match decision {
                                    hello_cli::diagnostics::RecognitionDecision::Recognized => serde_json::Value::Null,
                                    hello_cli::diagnostics::RecognitionDecision::NotRecognized => "active_threshold_not_met".into(),
                                    hello_cli::diagnostics::RecognitionDecision::Inconclusive => "frame_scores_disagree_with_threshold".into(),
                                };
                            }
                            report["experimental"] = true.into();
                            report["threshold_calibrated_for_camera"] = false.into();
                            let name = reference.lock().expect("session reference mutex").display_name().unwrap_or("Saved person").to_owned();
                            let (title, state) = named_recognition_result(decision, &name);
                            ui.set_recognition_title(title.into());
                            ui.set_recognition_state(state);
                            recognition_message(decision, summary.minimum(), threshold.value())
                        } else {
                            format!("3 fresh samples · cosine median {:.3}, range {:.3} to {:.3}. No identity or login verdict.", summary.median(), summary.minimum(), summary.maximum())
                        };
                        (message, serde_json::to_string_pretty(&report).unwrap())
                    },
                    Err(error) => {
                        let mut failed=capture_report;
                        failed["recognition_outcome"]=if cancel.load(Ordering::Acquire) {"cancelled"} else {"acquisition_failed"}.into();
                        failed["active_threshold"]=serde_json::to_value(threshold.map(|t|t.value())).unwrap();
                        failed["failure_reason"]=error.to_string().into();
                        (format!("Comparison stopped: {error}. No complete result."),serde_json::to_string_pretty(&failed).unwrap())
                    },
                };
                    if let Some(trials) = trial_message { ui.set_trial_message(format!("{trials}\n{message}").into()); }
                    ui.set_diagnostics(format!("LAST FACE COMPARISON\n{report}\n\nRESULT\n{message}").into());
                    ui.set_face_check_message(message.into());
                    if action.is_recognition() && ui.get_recognition_state() == 5 {
                        ui.set_recognition_state(4);
                        ui.set_recognition_title("Attempt interrupted — try again".into());
                    }
                    ui.set_face_check_running(false);
                    ui.set_scan_frame_available(false);
                    ui.set_preview_image(slint::Image::default());
                });
                return;
            }
            let result = if action == FaceAction::AddSample {
                hello_cli::diagnostics::inspect_sample_observed(executable, device.path, models, ir, Some(device.identity.clone()), cancel.clone(), Some(observer)).await
            } else {
                hello_cli::diagnostics::inspect_face(executable, device.path, models, ir, Some(device.identity.clone()), cancel.clone()).await
            };
            display_alive.store(false, Ordering::Release);
            let probe_report = result.as_ref().ok().and_then(|probe| serde_json::to_string_pretty(&probe.report).ok());
            let mut saved = false;
            let mut samples = None;
            let message = match result {
                Ok(_) if cancel.load(Ordering::Acquire) => "Face check cancelled.".into(),
                Ok(probe) if probe.report["diagnostic_passed"] != true => face_check_message(&probe.report),
                Ok(probe) => {
                    let mut reference = reference.lock().expect("session reference mutex");
                    if action == FaceAction::AddSample {
                        match reference.add_sample(&device.identity, ir, probe) {
                            Ok(count) => { saved = true; samples = Some(count as i32); if count == hello_cli::diagnostics::REFERENCE_SAMPLES { "Ready! You can verify this person or save the reference for later.".into() } else { format!("Sample {count}/{} accepted. {}", hello_cli::diagnostics::REFERENCE_SAMPLES, person_instruction(count)) } },
                            Err(error) => format!("Sample rejected: {error}. {}", person_instruction(reference.enrollment_sample_count())),
                        }
                    } else if action == FaceAction::Remember {
                        match reference.remember(device.identity, ir, probe) {
                            Ok(()) => { saved = true; samples = Some(0); "Face reference remembered for this window only. Use Check face to compare; no login enabled.".into() },
                            Err(error) => format!("Reference rejected: {error}"),
                        }
                    } else {
                        match reference.compare(&device.identity, ir, &probe) {
                            Ok(Some(score)) => comparison_message(score),
                            Ok(None) => face_check_message(&probe.report),
                            Err(error) => format!("Comparison unavailable: {error}"),
                        }
                    }
                },
                Err(error) => format!("Face check stopped: {error}"),
            };
            let template_present = reference.lock().expect("session reference mutex").template_available();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                if let Some(report) = probe_report {
                    ui.set_diagnostics(format!("LAST FACE CHECK\n{report}\n\nRESULT\n{message}\n\nRefresh reloads camera and service metadata.").into());
                }
                ui.set_template_present(template_present);
                if let Some(count) = samples { ui.set_sample_count(count); }
                if saved { ui.set_reference_present(true); }
                if action == FaceAction::AddSample || action == FaceAction::Remember {
                    sync_person_ui(&ui, &reference.lock().expect("session reference mutex"));
                }
                ui.set_face_check_running(false);
                ui.set_scan_frame_available(false);
                ui.set_preview_image(slint::Image::default());
                ui.set_face_check_message(message.into());
            });
        });
        *active_job.lock().expect("active camera job mutex") = Some(job);
    });
    let check = capture.clone();
    window.on_check_face(move || check(FaceAction::Check));
    let compare = capture.clone();
    window.on_compare_three(move || compare(FaceAction::CompareThree));
    let recognize = capture.clone();
    window.on_recognize_face(move || recognize(FaceAction::Recognize));
    let motion = capture.clone();
    window.on_test_motion(move || motion(FaceAction::MotionDiagnostic));
    let saved_trial = capture.clone();
    window.on_test_saved_person(move || saved_trial(FaceAction::TestSavedPerson));
    let other_trial = capture.clone();
    window.on_test_other_person(move || other_trial(FaceAction::TestOtherPerson));
    let remember = capture.clone();
    window.on_remember_face(move || remember(FaceAction::Remember));
    let next_sample = capture.clone();
    window.on_add_sample(move || next_sample(FaceAction::AddSample));
    let scan = capture.clone();
    let scan_reference = person_reference.clone();
    let scan_devices = person_devices.clone();
    let scan_ui = window.as_weak();
    window.on_scan_person(move || {
        let Some(ui) = scan_ui.upgrade() else {
            return;
        };
        if ui.get_face_check_running() || ui.get_preview_running() || ui.get_refreshing() {
            return;
        }
        if !ui.get_enrollment_active() {
            let device = scan_devices.lock().ok().and_then(|list| {
                usize::try_from(ui.get_selected_device())
                    .ok()
                    .and_then(|index| list.get(index).cloned())
            });
            let Some(device) = device else {
                ui.set_face_check_message("Choose a camera and select Scan face.".into());
                return;
            };
            let mut reference = scan_reference.lock().expect("session reference mutex");
            if let Err(error) =
                reference.begin_anonymous_enrollment(&device.identity, ui.get_use_ir_emitter())
            {
                ui.set_face_check_message(error.to_string().into());
                return;
            }
            sync_person_ui(&ui, &reference);
        }
        ui.set_recognition_state(0);
        scan(FaceAction::ScanPerson);
    });
    configure_person_actions(window, person_reference, person_devices, capture);
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn sync_person_ui(ui: &SettingsWindow, reference: &hello_cli::diagnostics::SessionReference) {
    ui.set_person_needs_name(reference.template_available() && reference.display_name().is_none());
    ui.set_enrollment_active(reference.enrollment_active());
    ui.set_reference_present(reference.has_reference());
    ui.set_template_present(reference.template_available());
    let count = if reference.enrollment_active() {
        reference.enrollment_sample_count()
    } else {
        reference.reference_sample_count()
    };
    ui.set_sample_count(count as i32);
    ui.set_sample_instruction(person_instruction(count).into());
    ui.set_active_person_name(reference.display_name().unwrap_or("").into());
    ui.set_person_name(
        if reference.enrollment_active() {
            reference.enrollment_display_name()
        } else {
            reference.display_name()
        }
        .unwrap_or("")
        .into(),
    );
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn scan_preview_observer(
    weak: slint::Weak<SettingsWindow>,
    cancel: Arc<AtomicBool>,
) -> (hello_cli::diagnostics::FrameObserver, Arc<AtomicBool>) {
    let alive = Arc::new(AtomicBool::new(true));
    let active = alive.clone();
    let pending = Arc::new(AtomicBool::new(false));
    let observer = Arc::new(move |frame: &hello_camera::Frame| {
        if !active.load(Ordering::Acquire)
            || cancel.load(Ordering::Acquire)
            || pending.swap(true, Ordering::AcqRel)
        {
            return;
        }
        let Ok(rgb) = frame.to_rgb() else {
            pending.store(false, Ordering::Release);
            return;
        };
        let rgb = zeroize::Zeroizing::new(rgb);
        let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
            &rgb,
            frame.width,
            frame.height,
        );
        let pending_ui = pending.clone();
        let active = active.clone();
        let cancel = cancel.clone();
        if weak
            .upgrade_in_event_loop(move |ui| {
                if active.load(Ordering::Acquire)
                    && !cancel.load(Ordering::Acquire)
                    && ui.get_face_check_running()
                {
                    ui.set_preview_image(slint::Image::from_rgb8(buffer));
                    ui.set_scan_frame_available(true);
                }
                pending_ui.store(false, Ordering::Release);
            })
            .is_err()
        {
            pending.store(false, Ordering::Release);
        }
    });
    (observer, alive)
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn configure_person_actions(
    window: &SettingsWindow,
    reference: Arc<Mutex<hello_cli::diagnostics::SessionReference>>,
    devices: Arc<Mutex<Vec<DeviceInfo>>>,
    capture: Rc<dyn Fn(FaceAction)>,
) {
    let weak = window.as_weak();
    let begin_reference = reference.clone();
    window.on_begin_person(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_face_check_running()
            || ui.get_preview_running()
            || ui.get_refreshing()
            || ui.get_enrollment_active()
        {
            return;
        }
        let device = devices.lock().ok().and_then(|list| {
            usize::try_from(ui.get_selected_device())
                .ok()
                .and_then(|index| list.get(index).cloned())
        });
        let Some(device) = device else {
            ui.set_face_check_message("Choose a camera first.".into());
            return;
        };
        {
            let mut reference = begin_reference.lock().expect("session reference mutex");
            if let Err(error) = reference.begin_named_enrollment(
                &device.identity,
                ui.get_use_ir_emitter(),
                ui.get_person_name().as_str(),
            ) {
                ui.set_recognition_state(0);
                ui.set_face_check_message(format!("Enter a valid name: {error}").into());
                return;
            }
            sync_person_ui(&ui, &reference);
        }
        ui.set_recognition_state(0);
        capture(FaceAction::AddSample);
    });
    let weak = window.as_weak();
    let cancel_reference = reference.clone();
    window.on_cancel_person(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_face_check_running() || ui.get_preview_running() {
            return;
        }
        let mut reference = cancel_reference.lock().expect("session reference mutex");
        reference.cancel_enrollment();
        sync_person_ui(&ui, &reference);
        ui.set_recognition_state(0);
        ui.set_face_check_message(if reference.has_reference() {
            "Enrollment cancelled. The previous reference is still available.".into()
        } else {
            "Enrollment cancelled. You can start a new scan.".into()
        });
    });
    let weak = window.as_weak();
    window.on_rename_person(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_face_check_running() || ui.get_preview_running() {
            return;
        }
        let mut reference = reference.lock().expect("session reference mutex");
        let was_unnamed = reference.display_name().is_none();
        match reference.rename(ui.get_person_name().as_str()) {
            Ok(()) => {
                sync_person_ui(&ui, &reference);
                ui.set_recognition_state(0);
                ui.set_face_check_message(if was_unnamed {
                    "Face ready. Choose where to use it and save the configuration.".into()
                } else {
                    "Name changed. Save the reference to keep this change after restarting.".into()
                });
                if was_unnamed {
                    ui.set_page(2);
                }
            }
            Err(error) => {
                ui.set_recognition_state(0);
                ui.set_face_check_message(format!("Cannot rename: {error}").into())
            }
        }
    });
}

#[cfg(any(test, all(feature = "face-diagnostics", target_os = "linux")))]
fn person_instruction(count: usize) -> &'static str {
    match count {
        0 => "Look straight at the camera and hold still.",
        1 => "Keep looking straight ahead; move a little closer with your whole face in view.",
        2 => {
            "Keep looking straight ahead; move back slightly while keeping your face large in the preview."
        }
        3 => "Turn slightly toward the left edge of the preview and hold still.",
        4 => "Turn slightly toward the right edge of the preview and hold still.",
        5 => "Raise your chin slightly.",
        6 => "Lower your chin slightly.",
        7 => "Look straight at the camera once more.",
        _ => "Eight samples ready — face enrolled.",
    }
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn named_recognition_result(
    decision: hello_cli::diagnostics::RecognitionDecision,
    name: &str,
) -> (String, i32) {
    use hello_cli::diagnostics::RecognitionDecision;
    match decision {
        RecognitionDecision::Recognized => (format!("Recognized: {name}"), 1),
        RecognitionDecision::NotRecognized => ("Saved person not recognized".into(), 2),
        RecognitionDecision::Inconclusive => ("Ambiguous result — try again".into(), 3),
    }
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn motion_progress_title(feedback: &hello_cli::diagnostics::MotionProgress) -> String {
    use hello_cli::diagnostics::MotionStep;
    if feedback.quality_rejected {
        return format!("{} · unclear sample, retrying", feedback.step.instruction());
    }
    match feedback.step {
        MotionStep::TurnImageLeft | MotionStep::TurnImageRight => format!(
            "{} · movement {}%",
            feedback.step.instruction(),
            feedback.percent
        ),
        _ => feedback.step.instruction().into(),
    }
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn parse_recognition_threshold(
    text: &str,
) -> Result<hello_cli::diagnostics::RecognitionThreshold, hello_cli::face_worker::Error> {
    let threshold = hello_cli::diagnostics::RecognitionThreshold::new(text.trim().parse()?)?;
    if threshold.value() < hello_cli::diagnostics::STRICT_THRESHOLD_FLOOR {
        return Err("Strict recognition requires a threshold of at least 0.80".into());
    }
    Ok(threshold)
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn recognition_message(
    decision: hello_cli::diagnostics::RecognitionDecision,
    minimum: f32,
    threshold: f32,
) -> String {
    use hello_cli::diagnostics::RecognitionDecision;
    let result = match decision {
        RecognitionDecision::Recognized => "Saved face recognized",
        RecognitionDecision::NotRecognized => "Face not recognized",
        RecognitionDecision::Inconclusive => "Inconclusive: captures disagree; try again",
    };
    format!(
        "{result} (experimental) · minimum {minimum:.3}, threshold {threshold:.3}. No login authorization."
    )
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum StorageAction {
    Save,
    SaveAndConfigure,
    Load,
    Delete,
}

#[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
fn configure_reference_storage(
    window: &SettingsWindow,
    runtime: &tokio::runtime::Runtime,
    reference: &Arc<Mutex<hello_cli::diagnostics::SessionReference>>,
    active_job: &Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
) {
    let weak = window.as_weak();
    let reference = reference.clone();
    let active_job = active_job.clone();
    let handle = runtime.handle().clone();
    let operation = Rc::new(move |action: StorageAction| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_face_check_running() || ui.get_preview_running() || ui.get_refreshing() {
            return;
        }
        let password = zeroize::Zeroizing::new(ui.get_reference_password().as_bytes().to_vec());
        let setup = action == StorageAction::SaveAndConfigure;
        let choices = [
            ui.get_use_for_sudo(),
            ui.get_use_for_login(),
            ui.get_use_for_unlock(),
        ];
        let confirmation =
            zeroize::Zeroizing::new(ui.get_reference_password_confirm().as_bytes().to_vec());
        ui.set_reference_password("".into());
        ui.set_reference_password_confirm("".into());
        if action != StorageAction::Delete
            && let Err(message) = validate_reference_password(
                &password,
                &confirmation,
                matches!(
                    action,
                    StorageAction::Save | StorageAction::SaveAndConfigure
                ),
            )
        {
            ui.set_storage_message(message.into());
            return;
        }
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(".local/share"))
            });
        let Some(base) = base.filter(|path| path.is_absolute()) else {
            ui.set_storage_message("An absolute user data directory is required.".into());
            return;
        };
        let path = base.join("linux-hello/diagnostic-reference");
        let setup_path = path.clone();
        let setup_password =
            zeroize::Zeroizing::new(if setup { password.to_vec() } else { Vec::new() });
        ui.set_face_check_running(true);
        ui.set_reference_io_running(true);
        ui.set_storage_message("Processing the encrypted reference…".into());
        if setup {
            ui.set_system_message(
                "Saving the face. A system authorization dialog will follow.".into(),
            );
        }
        let reference = reference.clone();
        let weak = weak.clone();
        let job = handle.spawn(async move {
            let ui_reference = reference.clone();
            let result = tokio::task::spawn_blocking(move || {
                let mut reference = reference.lock().expect("session reference mutex");
                let result = match action {
                    StorageAction::Save | StorageAction::SaveAndConfigure => reference.save(&path, &password).map(|()| "Reference saved.".to_string()),
                    StorageAction::Load => reference.load(&path, &password).map(|()| "Reference loaded. Open the Face tab and select the original camera and IR mode.".to_string()),
                    StorageAction::Delete => reference.delete_saved(&path).map(|removed| if removed { "Saved reference deleted; memory reference cleared.".into() } else { "No saved reference found; memory reference cleared.".into() }),
                };
                result.map(|message| (message, reference.template_available(), reference.reference_sample_count() as i32))
            }).await;
            let (message, template, count, success) = match result {
                Ok(Ok((message, template, count))) => (message, template, count, true),
                Ok(Err(error)) => (format!("Reference operation failed: {error}. Check the passphrase and storage permissions."), false, 0, false),
                Err(error) => (format!("Reference worker failed: {error}"), false, 0, false),
            };
            let mut configured = None;
            if success && setup {
                let setup_ui = weak.clone();
                configured = Some(tokio::task::spawn_blocking(move || system_setup::configure(&setup_path, &setup_password, choices, setup_ui)).await);
            }
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_face_check_running(false);
                ui.set_reference_io_running(false);
                ui.set_storage_message(message.into());
                if let Some(result) = configured {
                    match result {
                        Ok(Ok(message)) => { ui.set_system_message(message.into()); ui.set_system_configured(true); }
                        Ok(Err(message)) => { ui.set_system_message(message.into()); ui.set_system_configured(false); }
                        Err(_) => { ui.set_system_message("Reference saved, but system setup did not finish. Try again.".into()); ui.set_system_configured(false); }
                    }
                }
                if success {
                    sync_person_ui(&ui, &ui_reference.lock().expect("session reference mutex"));
                    if !matches!(action, StorageAction::Save | StorageAction::SaveAndConfigure) { ui.set_recognition_state(0); }
                    ui.set_template_present(template);
                    ui.set_reference_present(template);
                    if action == StorageAction::Load { ui.set_sample_count(count); }
                    if action == StorageAction::Delete {
                        ui.set_sample_count(0);
                        ui.set_face_check_message("Reference forgotten and saved file deleted.".into());
                    }
                }
            });
        });
        *active_job.lock().expect("active camera job mutex") = Some(job);
    });
    let save = operation.clone();
    window.on_save_reference(move || save(StorageAction::Save));
    let setup = operation.clone();
    window.on_save_and_configure(move || setup(StorageAction::SaveAndConfigure));
    let load = operation.clone();
    window.on_load_reference(move || load(StorageAction::Load));
    window.on_delete_reference(move || operation(StorageAction::Delete));
}

#[cfg(any(test, all(feature = "face-diagnostics", target_os = "linux")))]
fn validate_reference_password(
    password: &[u8],
    confirmation: &[u8],
    saving: bool,
) -> Result<(), &'static str> {
    if password.len() > 1024
        || std::str::from_utf8(password).map_or(true, |p| p.chars().count() < 12)
    {
        return Err(
            "Choose a separate reference passphrase: at least 12 characters, at most 1024 bytes.",
        );
    }
    if saving && password != confirmation {
        return Err("The reference passphrases do not match. Enter both again.");
    }
    Ok(())
}

#[cfg(any(test, all(feature = "face-diagnostics", target_os = "linux")))]
fn face_check_message(report: &serde_json::Value) -> String {
    if report["worker_isolated"] != true
        || report["authentication_ready"] != false
        || report["raw_frames_saved"] != false
        || report["embedding_saved"] != false
    {
        return "Face check failed: invalid diagnostic result.".into();
    }
    if report["diagnostic_passed"] == true && report["faces"] == 1 {
        "One face detected; image quality and model checks passed. No enrollment performed.".into()
    } else {
        format!(
            "Face check did not pass: {}. Try facing the camera with the lens uncovered.",
            report["rejection"]
                .as_str()
                .unwrap_or("one suitable face is required")
        )
    }
}

#[cfg(any(test, all(feature = "face-diagnostics", target_os = "linux")))]
fn comparison_message(score: f32) -> String {
    format!(
        "One face detected · similarity to session reference: {score:.3} (cosine −1 to 1). No identity or login verdict."
    )
}

async fn run_preview(
    config: CaptureConfig,
    weak: slint::Weak<SettingsWindow>,
    cancel: Arc<AtomicBool>,
    ir_emitter: bool,
) -> Result<(), hello_camera::CameraError> {
    let emitter = if ir_emitter {
        Some(hello_camera::IrEmitterLease::acquire(&config.device)?)
    } else {
        None
    };
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
    if let Some(emitter) = emitter {
        emitter.restore()?;
    }
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
        ui.set_face_check_available(true);
        ui.set_system_controls_available(true);
        ui.set_health_check_available(true);
        ui.set_models_directory("/local/models".into());
        ui.set_face_check_message(
            "Select a camera and check face detection. This does not enroll you or enable login."
                .into(),
        );
        ui.show().unwrap();
        for (width, height) in [(980, 740), (760, 660)] {
            backend.set_size(slint::PhysicalSize::new(width, height));
            let mut header: Option<Vec<slint::Rgba8Pixel>> = None;
            for page in 0..5 {
                ui.set_page(page);
                let pixels = ui.window().take_snapshot().unwrap();
                assert_eq!(pixels.width(), width);
                assert_eq!(pixels.height(), height);
                assert!(
                    pixels.as_slice().windows(2).any(|pair| pair[0] != pair[1]),
                    "UI must contain rendered content"
                );
                let current_header = &pixels.as_slice()[..width as usize * 170];
                if let Some(header) = &header {
                    assert!(
                        current_header == header.as_slice(),
                        "page {page} must preserve the shared header at width {width}"
                    );
                } else {
                    header = Some(current_header.to_vec());
                }
                save_ui_snapshot(&pixels, &format!("page-{page}-{width}"));
            }
        }
        for (width, height) in [(980, 740), (760, 660)] {
            backend.set_size(slint::PhysicalSize::new(width, height));
            ui.set_page(1);
            ui.set_person_name("Zoë Morgan".into());
            ui.set_active_person_name("Zoë Morgan".into());
            for state in [
                "adding",
                "scanning",
                "naming",
                "ready",
                "recognized",
                "unknown",
            ] {
                ui.set_enrollment_active(matches!(state, "adding" | "scanning"));
                ui.set_face_check_running(state == "scanning");
                ui.set_person_needs_name(state == "naming");
                ui.set_template_present(!matches!(state, "adding" | "scanning"));
                ui.set_reference_present(!matches!(state, "adding" | "scanning"));
                ui.set_sample_count(if matches!(state, "adding" | "scanning") {
                    3
                } else {
                    8
                });
                ui.set_sample_instruction(person_instruction(3).into());
                ui.set_recognition_state(match state {
                    "recognized" => 1,
                    "unknown" => 2,
                    _ => 0,
                });
                ui.set_recognition_title(
                    if state == "recognized" {
                        "Recognized: Zoë Morgan"
                    } else {
                        "Saved person not recognized"
                    }
                    .into(),
                );
                ui.set_face_check_message(
                    "Samples remain local. System authentication is not enabled by this diagnostic test.".into(),
                );
                let pixels = ui.window().take_snapshot().unwrap();
                assert_eq!((pixels.width(), pixels.height()), (width, height));
                save_ui_snapshot(&pixels, &format!("person-{state}-{width}"));
            }
        }
        ui.hide().unwrap();
    }
    fn save_ui_snapshot(pixels: &slint::SharedPixelBuffer<slint::Rgba8Pixel>, label: &str) {
        if let Some(directory) = std::env::var_os("LINUX_HELLO_UI_SNAPSHOT_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            let path = std::path::Path::new(&directory).join(format!("{label}.ppm"));
            let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
            write!(file, "P6\n{} {}\n255\n", pixels.width(), pixels.height()).unwrap();
            for pixel in pixels.as_slice() {
                file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::{comparison_message, face_check_message, validate_reference_password};
    #[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
    use super::{named_recognition_result, parse_recognition_threshold, recognition_message};

    #[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
    #[test]
    fn named_result_shows_the_person_only_for_a_positive_match() {
        use hello_cli::diagnostics::RecognitionDecision;
        assert_eq!(
            named_recognition_result(RecognitionDecision::Recognized, "Jan"),
            ("Recognized: Jan".into(), 1)
        );
        for decision in [
            RecognitionDecision::NotRecognized,
            RecognitionDecision::Inconclusive,
        ] {
            let (title, state) = named_recognition_result(decision, "Jan");
            assert!(!title.contains("Jan"));
            assert_ne!(state, 1);
        }
    }

    #[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
    #[test]
    fn invalid_recognition_thresholds_are_rejected_before_capture() {
        for text in [
            "", "wrong", "NaN", "inf", "-0.5", "0", "0.363", "0.79", "1.01",
        ] {
            assert!(parse_recognition_threshold(text).is_err(), "{text}");
        }
        assert_eq!(parse_recognition_threshold(" 0.80 ").unwrap().value(), 0.80);
        assert_eq!(parse_recognition_threshold("1").unwrap().value(), 1.0);
    }

    #[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
    #[test]
    fn recognition_results_distinguish_mismatch_from_ambiguity_and_remain_experimental() {
        use hello_cli::diagnostics::RecognitionDecision;
        for (decision, prefix) in [
            (RecognitionDecision::Recognized, "Saved face recognized"),
            (RecognitionDecision::NotRecognized, "Face not recognized"),
            (RecognitionDecision::Inconclusive, "Inconclusive"),
        ] {
            let message = recognition_message(decision, 0.7, 0.363);
            assert!(message.starts_with(prefix));
            assert!(message.contains("minimum 0.700"));
            assert!(message.contains("experimental"));
            assert!(message.contains("No login authorization"));
        }
    }

    #[cfg(all(feature = "face-diagnostics", target_os = "linux"))]
    #[test]
    fn baseline_and_quality_feedback_do_not_claim_zero_measured_movement() {
        use hello_cli::diagnostics::{MotionProgress, MotionStep};
        let mut feedback = MotionProgress {
            step: MotionStep::Front,
            percent: 0,
            hint: "Baseline: 1/2".into(),
            quality_rejected: false,
        };
        let title = super::motion_progress_title(&feedback);
        assert!(title.contains("baseline"));
        assert!(!title.contains('%'));
        assert!(!title.contains("movement"));
        feedback.step = MotionStep::TurnImageLeft;
        feedback.quality_rejected = true;
        let title = super::motion_progress_title(&feedback);
        assert!(title.contains("unclear"));
        assert!(title.starts_with(feedback.step.instruction()));
        assert!(!title.contains('%'));
        feedback.quality_rejected = false;
        feedback.percent = 70;
        assert!(super::motion_progress_title(&feedback).contains("movement 70%"));
    }

    #[test]
    fn saved_reference_requires_length_and_confirmation_but_unlock_does_not() {
        assert!(validate_reference_password(b"short", b"short", true).is_err());
        assert!(
            validate_reference_password(b"test reference phrase", b"different phrase", true)
                .is_err()
        );
        assert!(
            validate_reference_password(b"test reference phrase", b"test reference phrase", true)
                .is_ok()
        );
        assert!(validate_reference_password(b"test reference phrase", b"", false).is_ok());
        assert!(validate_reference_password(&vec![b'x'; 1025], b"", false).is_err());
    }

    #[test]
    fn similarity_is_displayed_as_a_score_without_an_identity_verdict() {
        for score in [-1.0, 0.0, 1.0] {
            let message = comparison_message(score);
            assert!(message.contains("No identity or login verdict"));
            assert!(message.contains(&format!("{score:.3}")));
        }
    }

    fn report() -> serde_json::Value {
        serde_json::json!({"worker_isolated":true,"authentication_ready":false,
            "raw_frames_saved":false,"embedding_saved":false,
            "diagnostic_passed":true,"faces":1,"rejection":null})
    }

    #[test]
    fn successful_check_explains_it_does_not_enroll() {
        assert_eq!(
            face_check_message(&report()),
            "One face detected; image quality and model checks passed. No enrollment performed."
        );
    }

    #[test]
    fn rejected_checks_do_not_claim_success() {
        for (faces, rejection) in [
            (0, "no face detected"),
            (2, "multiple faces"),
            (1, "image quality too low"),
        ] {
            let mut report = report();
            report["faces"] = faces.into();
            report["diagnostic_passed"] = false.into();
            report["rejection"] = rejection.into();
            assert!(face_check_message(&report).starts_with("Face check did not pass:"));
            assert!(face_check_message(&report).contains(rejection));
        }
    }

    #[test]
    fn invalid_security_metadata_is_rejected() {
        for (field, bad) in [
            ("worker_isolated", false),
            ("authentication_ready", true),
            ("raw_frames_saved", true),
            ("embedding_saved", true),
        ] {
            let mut report = report();
            report[field] = bad.into();
            assert!(face_check_message(&report).contains("invalid diagnostic result"));
            report.as_object_mut().unwrap().remove(field);
            assert!(face_check_message(&report).contains("invalid diagnostic result"));
        }
    }
}
