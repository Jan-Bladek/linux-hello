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
    /// Capture a bounded set of frames; report metadata only, never save pixels.
    Capture {
        #[arg(long)]
        device: PathBuf,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..=300))]
        frames: u32,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=30))]
        timeout: u64,
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
}

fn output(value: &impl Serialize) -> Result<(), Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
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
    hardware_profile: Option<&'static str>,
    conclusion: &'static str,
}

async fn capture(path: PathBuf, frames: u32, timeout: u64, probe: bool) -> Result<(), Error> {
    let devices = tokio::task::spawn_blocking(hello_camera::enumerate).await??;
    let device = devices
        .into_iter()
        .find(|d| d.path == path)
        .ok_or("device was not found in V4L2 enumeration; use the exact reported path")?;
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
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => { interrupted = true; break; },
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
        hardware_profile: probe
            .then_some("Lenovo Yoga Slim 7 14IMH9 (operator-selected; model not auto-detected)"),
        conclusion: "Capture and RGB conversion passed. IR emitter, PAD, biometric accuracy, unplug/replug, and login compatibility remain unvalidated.",
    })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    match Arguments::parse().command {
        Command::Cameras => output(&tokio::task::spawn_blocking(hello_camera::enumerate).await??),
        Command::Capture {
            device,
            frames,
            timeout,
        } => capture(device, frames, timeout, false).await,
        Command::Probe { device } => capture(device, 30, 15, true).await,
        Command::Status { session } => output(&hello_ipc::status(session).await?),
        Command::Security => output(&serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "security": hello_core::SecurityStatus::default(),
            "pam_blockers": hello_pam::blockers(),
        })),
        Command::Fingerprints => output(&hello_fingerprint::readers().await?),
    }
}
