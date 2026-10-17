//! Explicit system authorization, followed by a bounded private setup pipe.
use crate::generated_ui::SettingsWindow;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const HELPER: &str = "/usr/local/libexec/linux-hello-sudo/configure-person.py";

fn configured_choices(data: &serde_json::Value, choices: [bool; 3]) -> bool {
    data["configured"]
        == serde_json::json!({"sudo":choices[0],"login":choices[1],"unlock":choices[2]})
}

pub fn configure(
    source: &std::path::Path,
    password: &[u8],
    choices: [bool; 3],
    ui: slint::Weak<SettingsWindow>,
) -> Result<String, String> {
    let source = source
        .to_str()
        .ok_or("The reference path is not valid text.")?;
    if !(12..=1024).contains(&password.len()) {
        return Err("Invalid reference passphrase length.".into());
    }
    let desktop_helper = "/usr/local/libexec/linux-hello-sudo/activate-desktop.py";
    let detected = Command::new("/usr/bin/python3")
        .args(["-B", desktop_helper, "--detect"])
        .output()
        .map_err(|_| "Cannot detect the desktop locker; check the installation.")?;
    let locker = String::from_utf8_lossy(&detected.stdout);
    let locker = locker.trim();
    if !detected.status.success() || !matches!(locker, "kde" | "hyprlock" | "login") {
        return Err("Cannot detect a supported desktop locker.".into());
    }
    let metadata = serde_json::json!({"source":source,"sudo":choices[0],"login":choices[1],"unlock":choices[2],"unlock_service":locker});
    let started = Instant::now();
    let deadline = started + Duration::from_secs(300);
    let mut child = Command::new("/usr/bin/pkexec")
        .arg(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Cannot open the system authorization dialog.".to_string())?;
    let input = child
        .stdin
        .take()
        .ok_or("Private setup channel unavailable.")?;
    let mut input = hello_cli::process_io::DeadlineIo::new(input, deadline)
        .map_err(|_| "Cannot configure the setup pipe deadline")?;
    let write_result = (|| -> std::io::Result<()> {
        serde_json::to_writer(&mut input, &metadata)?;
        input.write_all(b"\n")?;
        input.write_all(&(password.len() as u16).to_be_bytes())?;
        input.write_all(password)?;
        Ok(())
    })();
    drop(input);
    if write_result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Authorization was cancelled or the setup helper is unavailable.".into());
    }
    let output = child
        .stdout
        .take()
        .ok_or("No response from the setup helper.")?;
    let reader = thread::spawn(move || {
        let Ok(output) = hello_cli::process_io::DeadlineIo::new(output, deadline) else {
            return (false, "Cannot configure the response deadline".into());
        };
        let mut reader = BufReader::new(output);
        let mut configured = false;
        let mut message = String::new();
        // Bounded fixed metadata. Never display raw helper output or biometrics.
        for _ in 0..64 {
            let mut line = String::new();
            use std::io::Read;
            match Read::by_ref(&mut reader).take(4097).read_line(&mut line) {
                Ok(0) => return (configured, message),
                Ok(_) => (),
                Err(_) => {
                    return (
                        false,
                        "Setup response timed out or failed; refresh account status.".into(),
                    );
                }
            }
            if line.len() > 4096 {
                return (false, "Oversized setup response".into());
            }
            let Ok(data) = serde_json::from_str::<serde_json::Value>(&line) else {
                return (false, "Malformed setup response".into());
            };
            if data.as_object().is_none_or(|object| {
                object.len() != 1
                    || (!object.contains_key("message") && !object.contains_key("configured"))
            }) {
                return (false, "Unexpected setup response".into());
            }
            if let Some(text) = data["message"].as_str() {
                message = text.chars().take(500).collect();
                let update = message.clone();
                let _ = ui.upgrade_in_event_loop(move |ui| ui.set_system_message(update.into()));
            }
            configured |= configured_choices(&data, choices);
        }
        (false, "Setup response message budget exceeded".into())
    });
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
        if started.elapsed() >= Duration::from_secs(300) {
            // Root setup has its own 240-second alarm and rollback. Do not kill
            // an authorized root transaction midway through recovery.
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(100));
    };
    let (configured, message) = reader.join().map_err(|_| "Cannot read the setup result.")?;
    if status.is_some_and(|s| s.success()) && configured {
        if choices[2] && locker == "login" {
            let result = Command::new("/usr/bin/python3")
                .args(["-B", desktop_helper, "--configure-noctalia"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|_| "Face configured. Enable Allow empty password in Noctalia Security settings to start face verification with Enter.")?;
            if !result.success() {
                return Ok("Face configured. Enable Allow empty password in Noctalia Security settings to start face verification with Enter.".into());
            }
        }
        Ok("Face saved and configured for the selected uses.".into())
    } else {
        Err(if message.is_empty() {
            "Setup was not approved. Previous authentication and password fallback remain available.".into()
        } else {
            message
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn success_requires_exact_completed_choices_not_progress_or_partial_setup() {
        let choices = [true, false, true];
        assert!(configured_choices(
            &serde_json::json!({"configured":{"sudo":true,"login":false,"unlock":true}}),
            choices
        ));
        for data in [
            serde_json::json!({"message":"done"}),
            serde_json::json!({"configured":{}}),
            serde_json::json!({"configured":{"sudo":true,"login":true,"unlock":true}}),
        ] {
            assert!(!configured_choices(&data, choices));
        }
    }
}
