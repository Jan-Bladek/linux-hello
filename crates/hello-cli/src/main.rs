use clap::{Parser, Subcommand};
use hello_camera::{CameraSession, CaptureConfig};
use serde::Serialize;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Parser)]
#[command(
    name = "hello",
    version,
    about = "Linux Hello: local camera and security diagnostics"
)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enumerate V4L2 devices, pixel formats, sizes, and IR hints without capture.
    Cameras,
    /// Inspect installation health without capturing frames or accessing biometrics.
    Doctor {
        /// Emit a sanitized JSON report suitable for a support issue.
        #[arg(long)]
        json: bool,
    },
    /// Authorized status, pause or removal for your own system face reference.
    #[cfg(target_os = "linux")]
    Account {
        #[arg(value_parser = ["status", "pause", "remove"])]
        action: String,
        /// Required before deleting the protected system reference.
        #[arg(long)]
        confirm: bool,
    },
    /// Capture a bounded set of frames; report metadata only, never save pixels.
    Capture {
        #[arg(long)]
        device: PathBuf,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..=300))]
        frames: u32,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=30))]
        timeout: u64,
        /// Use the tested Chicony IR face mode for this session, then restore it.
        #[arg(long)]
        ir_emitter: bool,
    },
    /// Run the initial Lenovo Yoga Slim 7 14IMH9 camera compatibility procedure.
    Probe {
        #[arg(long)]
        device: PathBuf,
    },
    /// Query the diagnostic daemon (fails if the daemon cannot be reached).
    Status {
        #[arg(long)]
        session: bool,
    },
    /// Show compiled security capabilities and blockers, without contacting a daemon.
    Security,
    /// Query readers and current-user enrollment through fprintd, without copying templates.
    Fingerprints,
    /// Inspect exposed V4L2 controls without setting them or starting capture.
    CameraControls {
        #[arg(long)]
        device: PathBuf,
    },
    /// Measure a bounded live frame series; report quality only, never save pixels.
    #[cfg(all(feature = "face-opencv", target_os = "linux"))]
    CameraQuality {
        #[arg(long)]
        device: PathBuf,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=300))]
        frames: u32,
        #[arg(long)]
        ir_emitter: bool,
    },
    /// Check an explicit local dataset without models, capture or inference.
    #[cfg(all(feature = "face-opencv", target_os = "linux"))]
    FaceDatasetCheck {
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Evaluate an explicit consented, identity-disjoint local dataset; no enrollment.
    #[cfg(all(feature = "face-opencv", target_os = "linux"))]
    FaceEvaluate {
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        target_far: f64,
    },
    /// Measure isolated inference using an explicitly selected local raw RGB file.
    #[cfg(all(feature = "face-opencv", target_os = "linux"))]
    FaceBenchmark {
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        rgb: PathBuf,
        #[arg(long)]
        width: u32,
        #[arg(long)]
        height: u32,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=30))]
        iterations: u32,
        /// Keep the restricted model worker loaded across iterations.
        #[arg(long)]
        stream: bool,
    },
    /// Evaluate one live frame locally with pinned YuNet/SFace models; no login verdict.
    #[cfg(all(feature = "face-opencv", target_os = "linux"))]
    FaceProbe {
        #[arg(long)]
        device: PathBuf,
        #[arg(long)]
        models: PathBuf,
        #[arg(long)]
        ir_emitter: bool,
    },
}

fn output(value: &impl Serialize) -> Result<(), Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Register both handlers before touching hardware, retaining receivers for the
/// whole operation. SIGTERM must clean up the emitter just like Ctrl+C.
fn termination_signal() -> Result<impl std::future::Future<Output = Result<(), Error>>, Error> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        Ok(async move {
            let received = tokio::select! {
                received = interrupt.recv() => received,
                received = terminate.recv() => received,
            };
            received.ok_or_else(|| "termination signal stream closed".into())
        })
    }
    #[cfg(not(unix))]
    {
        Ok(async {
            tokio::signal::ctrl_c().await?;
            Ok(())
        })
    }
}

fn restore_emitter(emitter: Option<hello_camera::IrEmitterLease>) -> Result<(), Error> {
    if let Some(emitter) = emitter {
        emitter.restore()?;
    }
    Ok(())
}

#[derive(Serialize)]
struct CaptureReport {
    requested_frames: u32,
    captured_frames: u32,
    width: u32,
    height: u32,
    elapsed_ms: u128,
    raw_frames_saved: bool,
    authentication_ready: bool,
    ir_face_mode_requested: bool,
    ir_face_mode_restored: bool,
    hardware_profile: Option<&'static str>,
    conclusion: &'static str,
}

async fn capture(
    path: PathBuf,
    frames: u32,
    timeout: u64,
    probe: bool,
    ir_emitter: bool,
) -> Result<(), Error> {
    let interrupt = termination_signal()?;
    tokio::pin!(interrupt);
    let devices = tokio::task::spawn_blocking(hello_camera::enumerate).await??;
    let device = devices
        .into_iter()
        .find(|d| d.path == path)
        .ok_or("device was not found in V4L2 enumeration; use the exact reported path")?;
    let emitter = if ir_emitter {
        Some(hello_camera::IrEmitterLease::acquire(&device)?)
    } else {
        None
    };
    let mut config = CaptureConfig::preview(device)?;
    config.max_frames = frames;
    config.timeout = Duration::from_secs(timeout);
    config.reconnect_attempts = 0;
    let started = Instant::now();
    let mut session = CameraSession::start(config)?;
    let mut count = 0;
    let mut dimensions = (0, 0);
    let mut interrupted = false;
    let result: Result<(), Error> = async {
        // Keep one listener across frames. Recreating ctrl_c() drops its signal
        // receiver whenever a frame wins select!, potentially losing SIGINT.
        loop {
            tokio::select! {
                biased;
                signal = &mut interrupt => { signal?; interrupted = true; break; },
                frame = session.next_frame() => match frame? {
                    Some(frame) => {
                        // Validate the preview path too; discard the pixels immediately.
                        let _rgb = frame.to_rgb()?;
                        dimensions = (frame.width, frame.height);
                        count += 1;
                    }
                    None => break,
                }
            }
        }
        Ok(())
    }
    .await;
    let stopped = session.stop().await;
    restore_emitter(emitter)?;
    result?;
    stopped?;
    if interrupted {
        return Err("capture cancelled; camera released".into());
    }
    if count != frames {
        return Err("capture ended before all requested frames arrived".into());
    }
    output(&CaptureReport {
        requested_frames: frames,
        captured_frames: count,
        width: dimensions.0,
        height: dimensions.1,
        elapsed_ms: started.elapsed().as_millis(),
        raw_frames_saved: false,
        authentication_ready: false,
        ir_face_mode_requested: ir_emitter,
        ir_face_mode_restored: ir_emitter,
        hardware_profile: probe
            .then_some("Lenovo Yoga Slim 7 14IMH9 (operator-selected; model not auto-detected)"),
        conclusion: "Capture and RGB conversion passed. IR emitter, PAD, biometric accuracy, unplug/replug, and login compatibility remain unvalidated.",
    })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    match Arguments::parse().command {
        Command::Doctor { json } => {
            let report = tokio::task::spawn_blocking(hello_cli::health::inspect).await?;
            if json {
                output(&report)
            } else {
                print!("{}", report.text());
                Ok(())
            }
        }
        #[cfg(target_os = "linux")]
        Command::Account { action, confirm } => {
            if action == "remove" && !confirm {
                return Err("Removal deletes your protected system reference. Use --confirm after reviewing this; password fallback and local GUI data remain available.".into());
            }
            let result = tokio::task::spawn_blocking(move || hello_cli::account::manage(&action))
                .await?
                .map_err(std::io::Error::other)?;
            output(&result)
        }
        Command::CameraControls { device } => {
            output(&tokio::task::spawn_blocking(move || hello_camera::controls(&device)).await??)
        }
        #[cfg(all(feature = "face-opencv", target_os = "linux"))]
        Command::CameraQuality {
            device,
            frames,
            ir_emitter,
        } => camera_quality(device, frames, ir_emitter).await,
        Command::Cameras => output(&tokio::task::spawn_blocking(hello_camera::enumerate).await??),
        Command::Capture {
            device,
            frames,
            timeout,
            ir_emitter,
        } => capture(device, frames, timeout, false, ir_emitter).await,
        Command::Probe { device } => capture(device, 30, 15, true, false).await,
        Command::Status { session } => output(&hello_ipc::status(session).await?),
        Command::Security => output(&serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "security": hello_core::SecurityStatus::default(),
            "pam_blockers": hello_pam::blockers(),
        })),
        Command::Fingerprints => output(&hello_fingerprint::readers().await?),
        #[cfg(all(feature = "face-opencv", target_os = "linux"))]
        Command::FaceDatasetCheck { manifest } => check_dataset(manifest).await,
        #[cfg(all(feature = "face-opencv", target_os = "linux"))]
        Command::FaceEvaluate {
            models,
            manifest,
            target_far,
        } => run_evaluation(models, Some((manifest, target_far)), None).await,
        #[cfg(all(feature = "face-opencv", target_os = "linux"))]
        Command::FaceBenchmark {
            models,
            rgb,
            width,
            height,
            iterations,
            stream,
        } => run_evaluation(models, None, Some((rgb, width, height, iterations, stream))).await,
        #[cfg(all(feature = "face-opencv", target_os = "linux"))]
        Command::FaceProbe {
            device,
            models,
            ir_emitter,
        } => face_probe(device, models, ir_emitter).await,
    }
}

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
async fn face_probe(path: PathBuf, models: PathBuf, ir_emitter: bool) -> Result<(), Error> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let interrupt = termination_signal()?;
    tokio::pin!(interrupt);
    let mut check = Box::pin(hello_cli::diagnostics::face_probe(
        std::env::current_exe()?.with_file_name("hello-face-worker"),
        path,
        models,
        ir_emitter,
        None,
        cancel.clone(),
    ));
    let report = tokio::select! {
        biased;
        signal = interrupt => {
            signal?;
            cancel.store(true, Ordering::Release);
            let _ = check.await;
            return Err("face probe cancelled; camera released and worker reaped".into());
        },
        report = &mut check => report?,
    };
    output(&report)?;
    if report["diagnostic_passed"] != true {
        return Err("face diagnostic rejected the frame; see report".into());
    }
    Ok(())
}

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
async fn camera_quality(path: PathBuf, frames: u32, ir_emitter: bool) -> Result<(), Error> {
    let interrupt = termination_signal()?;
    tokio::pin!(interrupt);
    let device = tokio::task::spawn_blocking(hello_camera::enumerate)
        .await??
        .into_iter()
        .find(|d| d.path == path)
        .ok_or("select an enumerated camera")?;
    let emitter = if ir_emitter {
        Some(hello_camera::IrEmitterLease::acquire(&device)?)
    } else {
        None
    };
    let mut config = CaptureConfig::preview(device)?;
    config.max_frames = frames;
    config.timeout = Duration::from_secs(15);
    let mut session = CameraSession::start(config)?;
    let result: Result<_, Error> = async {
        let mut records = Vec::new();
        loop {
            let captured = tokio::select! {
                biased;
                signal = &mut interrupt => { signal?; return Err("quality capture cancelled; camera released".into()); },
                captured = session.next_frame() => captured?,
            };
            let Some(captured) = captured else { break; };
            let quality = captured_quality(&captured)?;
            records.push(serde_json::json!({"sequence": captured.sequence, "width": captured.width,
                "height": captured.height, "format": captured.pixel_format.fourcc(),
                "quality": quality, "acceptable": quality.acceptable()}));
        }
        Ok(records)
    }.await;
    let stopped = session.stop().await;
    restore_emitter(emitter)?;
    let records = result?;
    stopped?;
    let passed = records.iter().filter(|r| r["acceptable"] == true).count();
    output(
        &serde_json::json!({"device": path, "requested_frames": frames, "observed_frames": records.len(),
        "acceptable_frames": passed, "raw_frames_saved": false, "authentication_ready": false,
        "spectrum_confirmed": false, "emitter_confirmed": false,
        "ir_face_mode_requested": ir_emitter, "ir_face_mode_restored": ir_emitter, "frames": records}),
    )?;
    if records.is_empty() {
        return Err("no quality frames acquired".into());
    }
    Ok(())
}

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
fn captured_quality(frame: &hello_camera::Frame) -> Result<hello_face::QualityMetrics, Error> {
    hello_cli::diagnostics::frame_quality(frame)
}

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
async fn check_dataset(manifest: PathBuf) -> Result<(), Error> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let task_cancel = cancel.clone();
    let mut task = tokio::task::spawn_blocking(move || {
        hello_cli::evaluation::dataset_check(&manifest, task_cancel)
    });
    let report = tokio::select! {
        biased;
        signal = termination_signal()? => {
            signal?;
            cancel.store(true, Ordering::Release);
            let _ = task.await;
            return Err("dataset check cancelled".into());
        },
        result = &mut task => result??,
    };
    output(&report)
}

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
async fn run_evaluation(
    models: PathBuf,
    dataset: Option<(PathBuf, f64)>,
    benchmark: Option<(PathBuf, u32, u32, u32, bool)>,
) -> Result<(), Error> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let executable = std::env::current_exe()?.with_file_name("hello-face-worker");
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let mut evaluation = tokio::task::spawn_blocking(move || {
        if let Some((path, target)) = dataset {
            hello_cli::evaluation::evaluate(&executable, &models, &path, target, worker_cancel)
        } else if let Some((rgb, width, height, iterations, stream)) = benchmark {
            let run = if stream {
                hello_cli::evaluation::benchmark_stream
            } else {
                hello_cli::evaluation::benchmark
            };
            run(
                &executable,
                &models,
                &rgb,
                width,
                height,
                iterations,
                worker_cancel,
            )
        } else {
            Err("missing evaluation mode".into())
        }
    });
    let report = tokio::select! {
        biased;
        signal = termination_signal()? => {
            signal?;
            cancel.store(true, Ordering::Release);
            let _ = evaluation.await;
            return Err("evaluation cancelled; worker reaped".into());
        },
        result = &mut evaluation => result??,
    };
    output(&report)?;
    if report["evaluation_available"] == false {
        return Err("dataset did not yield usable calibration/evaluation pairs".into());
    }
    Ok(())
}
