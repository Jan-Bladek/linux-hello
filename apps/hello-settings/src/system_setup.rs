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
    let metadata = serde_json::json!({"source":source,"sudo":choices[0],"login":choices[1],"unlock":choices[2]});
    let mut child = Command::new("/usr/bin/pkexec")
        .arg(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Cannot open the system authorization dialog.".to_string())?;
    let mut input = child
        .stdin
        .take()
        .ok_or("Private setup channel unavailable.")?;
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
        let mut reader = BufReader::new(output);
        let mut configured = false;
        let mut message = String::new();
        // Bounded fixed metadata. Never display raw helper output or biometrics.
        for _ in 0..64 {
            let mut line = String::new();
            use std::io::Read;
            if Read::by_ref(&mut reader)
                .take(4097)
                .read_line(&mut line)
                .unwrap_or(0)
                == 0
            {
                break;
            }
            if line.len() > 4096 {
                break;
            }
            let Ok(data) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if let Some(text) = data["message"].as_str() {
                message = text.chars().take(500).collect();
                let update = message.clone();
                let _ = ui.upgrade_in_event_loop(move |ui| ui.set_system_message(update.into()));
            }
            configured |= configured_choices(&data, choices);
        }
        (configured, message)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(_) => break None,
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
