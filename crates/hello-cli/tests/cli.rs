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
