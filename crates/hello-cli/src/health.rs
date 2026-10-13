//! Read-only, shareable installation diagnostics. Never captures or authenticates.
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub struct Check {
    name: &'static str,
    status: &'static str,
    detail: String,
}
#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    version: &'static str,
    checks: Vec<Check>,
    camera_capture_started: bool,
    biometric_data_included: bool,
    authentication_verdict: bool,
}
impl Report {
    pub fn text(&self) -> String {
        let mut text = format!("Linux Hello {} — installation health\n", self.version);
        for check in &self.checks {
            text.push_str(&format!(
                "[{}] {}: {}\n",
                check.status, check.name, check.detail
            ));
        }
        text.push_str(
            "Metadata only. No camera capture, biometric data or authentication verdict.\n",
        );
        text
    }
}
fn add(checks: &mut Vec<Check>, name: &'static str, ok: bool, detail: impl Into<String>) {
    checks.push(Check {
        name,
        status: if ok { "ok" } else { "check" },
        detail: detail.into(),
    });
}
pub fn inspect() -> Report {
    let mut checks = Vec::new();
    add(
        &mut checks,
        "platform",
        cfg!(target_os = "linux"),
        "Linux V4L2 is required; Fedora KDE is the tested target.",
    );
    match hello_camera::enumerate() {
        Ok(devices) => {
            let available = devices.iter().filter(|d| d.capture_supported).count();
            add(
                &mut checks,
                "cameras",
                available > 0,
                format!(
                    "{} device nodes; {} capture-capable. Enumeration only; no stream started.",
                    devices.len(),
                    available
                ),
            );
        }
        Err(_) => add(
            &mut checks,
            "cameras",
            false,
            "Enumeration unavailable; check session device permissions.",
        ),
    }
    let base = Path::new("/usr/local/libexec/linux-hello-sudo");
    for (name, path) in [
        ("system helper", base.join("hello-sudo")),
        ("account management", base.join("manage-person.py")),
        ("setup helper", base.join("configure-person.py")),
        ("PAM module", "/usr/lib64/security/pam_linuxhello.so".into()),
        ("authorization", "/usr/bin/pkexec".into()),
        ("TPM credential tool", "/usr/bin/systemd-creds".into()),
    ] {
        add(
            &mut checks,
            name,
            path.is_file(),
            if path.is_file() {
                "Installed file present; active account state requires an authorized status check."
            } else {
                "Missing; run the full source installer."
            },
        );
    }
    let worker = std::env::current_exe()
        .ok()
        .map(|p| p.with_file_name("hello-face-worker"));
    add(
        &mut checks,
        "inference worker",
        worker.is_some_and(|p| p.is_file()),
        "The worker must be adjacent to the GUI/CLI binary.",
    );
    #[cfg(all(feature = "face-opencv", target_os = "linux"))]
    {
        let installed = base.join("models");
        let models = if installed.is_dir() {
            installed
        } else {
            "work/inference/models".into()
        };
        add(
            &mut checks,
            "models",
            hello_face::opencv_engine::PinnedModels::read(&models).is_ok(),
            "Only the pinned YuNet/SFace byte sizes and SHA256 digests are accepted. No inference executed.",
        );
    }
    #[cfg(not(all(feature = "face-opencv", target_os = "linux")))]
    add(
        &mut checks,
        "models",
        false,
        "Build with face-opencv or use the full installer to validate model artifacts.",
    );
    let enforcing =
        std::fs::read_to_string("/sys/fs/selinux/enforce").is_ok_and(|s| s.trim() == "1");
    add(
        &mut checks,
        "SELinux",
        enforcing,
        if enforcing {
            "Enforcing."
        } else {
            "Enforcing mode not detected; inspect the host policy."
        },
    );
    add(
        &mut checks,
        "TPM device",
        Path::new("/dev/tpmrm0").exists(),
        "Device presence is not a key-release test or boot attestation.",
    );
    add(
        &mut checks,
        "security qualification",
        false,
        "Representative NIR FAR/FRR, reviewed PAD evidence and deployment qualification remain open.",
    );
    Report {
        schema_version: 1,
        version: env!("CARGO_PKG_VERSION"),
        checks,
        camera_capture_started: false,
        biometric_data_included: false,
        authentication_verdict: false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_format_explicitly_excludes_biometrics_and_authentication() {
        let report = Report {
            schema_version: 1,
            version: "test",
            checks: vec![],
            camera_capture_started: false,
            biometric_data_included: false,
            authentication_verdict: false,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["biometric_data_included"], false);
        assert_eq!(json["authentication_verdict"], false);
        assert_eq!(json["camera_capture_started"], false);
        assert!(report.text().contains("No camera capture"));
    }
}
