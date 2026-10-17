//! Minimal setuid launcher: no input protocol, only real caller's verification.
use hello_pam::boundary;
use std::{io::Read, os::unix::process::CommandExt, path::Path, process::Command};

fn service(args: &[String]) -> Option<&str> {
    match args {
        [] => Some("kde"), // retain the existing launcher interface
        [flag, name]
            if flag == "--service" && matches!(name.as_str(), "kde" | "hyprlock" | "login") =>
        {
            Some(name)
        }
        _ => None,
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let service = service(&args).ok_or("unsupported locker service or arguments")?;
    // SAFETY: UID getters take no pointers.
    let uid = unsafe { libc::getuid() };
    // SAFETY: geteuid takes no pointers.
    if uid < 1000 || unsafe { libc::geteuid() } != 0 {
        return Err("caller or arguments rejected".into());
    }
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: initialized limit and constant prctl arguments.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err("core limit failed".into());
    }
    // SAFETY: constant prctl arguments, no pointers.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err("dump protection failed".into());
    }
    let marker = Path::new(boundary::ROOT)
        .join(uid.to_string())
        .join(format!("pam-{service}.enabled"));
    let mut bytes = Vec::new();
    boundary::protected_file(&marker, true, 64)?
        .take(65)
        .read_to_end(&mut bytes)?;
    if bytes != b"linux-hello-login-v1\n" {
        return Err("not enabled".into());
    }
    boundary::protected_file(Path::new(boundary::HELPER), false, 100_000_000)?;
    boundary::protected_file(
        &Path::new(boundary::ROOT)
            .join(uid.to_string())
            .join("config.json"),
        true,
        16384,
    )?;
    // No inherited user descriptors can become privileged worker inputs. Keep
    // real UID so the unprivileged PAM parent can cancel its own child.
    // SAFETY: close_range with constant descriptor bounds and flags.
    if unsafe { libc::close_range(3, u32::MAX, 0) } != 0 {
        return Err("descriptor isolation failed".into());
    }
    let error = Command::new(boundary::HELPER)
        .args(["verify-service", &uid.to_string(), service])
        .env_clear()
        .env("LANG", "C.UTF-8")
        .current_dir("/")
        .exec();
    Err(error.into())
}
fn main() {
    if run().is_err() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_fixed_locker_services_are_selectable_never_a_target_or_command() {
        assert_eq!(service(&[]), Some("kde"));
        for name in ["kde", "hyprlock", "login"] {
            assert_eq!(service(&["--service".into(), name.into()]), Some(name));
        }
        for args in [
            vec!["verify", "1001"],
            vec!["--service", "sshd"],
            vec!["--service", "../login"],
            vec!["--service", "login", "1001"],
            vec!["1001"],
            vec!["--service", "sudo"],
        ] {
            assert!(service(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_none());
        }
    }
}
