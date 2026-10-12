use std::process::Command;

#[test]
fn security_output_is_machine_readable_and_authentication_disabled() {
    let result = Command::new(env!("CARGO_BIN_EXE_hello"))
        .arg("security")
        .output()
        .unwrap();
    assert!(result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        report["security"]["privileged_authentication_enabled"],
        false
    );
    assert_eq!(report["security"]["model_ready"], false);
    assert!(!report["pam_blockers"].as_array().unwrap().is_empty());
}

#[test]
fn unbounded_capture_input_is_rejected_before_device_access() {
    for (argument, value) in [("--frames", "0"), ("--frames", "301"), ("--timeout", "31")] {
        let result = Command::new(env!("CARGO_BIN_EXE_hello"))
            .args([
                "capture",
                "--device",
                "/dev/never-open-this",
                argument,
                value,
            ])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn missing_daemon_never_returns_successful_status() {
    let result = Command::new("dbus-run-session")
        .args(["--", env!("CARGO_BIN_EXE_hello"), "status", "--session"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires LINUX_HELLO_CAMERA and explicit physical capture"]
fn selected_camera_cli_cancels_and_reopens() {
    use std::{
        io::Read,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    let device = std::env::var("LINUX_HELLO_CAMERA")
        .expect("set LINUX_HELLO_CAMERA to an explicitly selected /dev/videoN");
    let suffix = device
        .strip_prefix("/dev/video")
        .expect("select /dev/videoN");
    assert!(!suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello"))
        .args([
            "capture",
            "--device",
            &device,
            "--frames",
            "300",
            "--timeout",
            "30",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(1500));
    let running = child.try_wait().unwrap().is_none();
    if running {
        let sent = Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap();
        if !sent.success() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("unable to send SIGINT to the test's capture process");
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(25));
    };
    let mut message = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut message)
        .unwrap();
    assert!(running, "capture exited before SIGINT: {message}");
    assert!(
        status.is_some(),
        "capture did not stop within five seconds of SIGINT"
    );
    assert_eq!(status.unwrap().code(), Some(1), "{message}");
    assert!(
        message.contains("capture cancelled; camera released"),
        "{message}"
    );
    let reopened = Command::new(env!("CARGO_BIN_EXE_hello"))
        .args([
            "capture",
            "--device",
            &device,
            "--frames",
            "5",
            "--timeout",
            "10",
        ])
        .output()
        .unwrap();
    assert!(
        reopened.status.success(),
        "{}",
        String::from_utf8_lossy(&reopened.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&reopened.stdout).unwrap();
    assert_eq!(report["captured_frames"], 5);
    assert_eq!(report["raw_frames_saved"], false);
}

#[cfg(all(feature = "face-opencv", target_os = "linux"))]
#[test]
fn invalid_face_models_fail_before_camera_access() {
    let models =
        std::env::temp_dir().join(format!("linux-hello-missing-models-{}", std::process::id()));
    let result = Command::new(env!("CARGO_BIN_EXE_hello"))
        .args(["face-probe", "--device", "/dev/never-open-this", "--models"])
        .arg(models)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains("ModelArtifact"));
    assert!(!error.contains("V4L2"));
}
