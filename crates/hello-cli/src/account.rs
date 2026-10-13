//! Bounded system authorization for the current account only; no biometric IPC.
use serde::Deserialize;
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub version: u32,
    pub uid: u32,
    pub enrolled: bool,
    pub sudo: bool,
    pub login: bool,
    pub unlock: bool,
    pub key_backend: String,
    pub operation: String,
}
impl Status {
    pub fn summary(&self) -> String {
        let state = |active| if active { "enabled" } else { "disabled" };
        format!(
            "System reference: {} · sudo: {} · KDE sign-in: {} · screen unlock: {} · key: {}",
            if self.enrolled {
                "configured"
            } else {
                "not enrolled"
            },
            state(self.sudo),
            state(self.login),
            state(self.unlock),
            self.key_backend
        )
    }
}
fn decode(bytes: &[u8], action: &str, uid: u32) -> Result<Status, String> {
    let result: Status = serde_json::from_slice(bytes)
        .map_err(|_| "Invalid or rejected account management response")?;
    if result.version != 1
        || result.uid != uid
        || result.operation != action
        || !["none", "file", "tpm"].contains(&result.key_backend.as_str())
        || (!result.enrolled && (result.sudo || result.login || result.unlock))
        || (action != "status" && (result.sudo || result.login || result.unlock))
        || (action == "remove" && result.enrolled)
    {
        return Err("Account management result did not match the authorized operation".into());
    }
    Ok(result)
}
pub fn manage(action: &str) -> Result<Status, String> {
    if !["status", "pause", "remove"].contains(&action) {
        return Err("Unknown account operation".into());
    }
    let uid = Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|_| "Cannot determine current UID")?;
    if !uid.status.success() {
        return Err("Cannot determine current UID".into());
    }
    let uid = String::from_utf8_lossy(&uid.stdout)
        .trim()
        .parse::<u32>()
        .map_err(|_| "Invalid current UID")?;
    if uid < 1000 {
        return Err("Run account management as your regular desktop user".into());
    }
    let mut child = Command::new("/usr/bin/pkexec")
        .args([
            "/usr/local/libexec/linux-hello-sudo/manage-person.py",
            action,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(
            |_| "Cannot start account authorization; check the full installation and Polkit agent",
        )?;
    let stdout = child.stdout.take().ok_or("Missing account response pipe")?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            _ => (),
        }
        if start.elapsed() >= Duration::from_secs(120) {
            // Root helper has its own 90-second cancellation/rollback alarm.
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(100));
    };
    let bytes = reader
        .join()
        .map_err(|_| "Account response reader failed")?
        .map_err(|_| "Cannot read account response")?;
    if bytes.len() > 4096 || !status.is_some_and(|s| s.success()) {
        return Err("Account operation was cancelled or failed. Refresh status; password fallback remains available.".into());
    }
    decode(&bytes, action, uid)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_result_requires_same_subject_action_and_completed_disable() {
        let good = r#"{"version":1,"uid":1000,"enrolled":true,"sudo":false,"login":false,"unlock":false,"key_backend":"tpm","operation":"pause"}"#;
        assert!(decode(good.as_bytes(), "pause", 1000).is_ok());
        assert!(decode(good.as_bytes(), "pause", 1001).is_err());
        assert!(decode(good.as_bytes(), "remove", 1000).is_err());
        assert!(
            decode(
                good.replace("\"sudo\":false", "\"sudo\":true").as_bytes(),
                "pause",
                1000
            )
            .is_err()
        );
        assert!(decode(b"{}", "status", 1000).is_err());
        assert!(decode(b"{\"error\":\"rejected\"}", "status", 1000).is_err());
    }
}
