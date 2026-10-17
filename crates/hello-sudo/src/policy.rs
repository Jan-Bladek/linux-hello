//! Narrow reviewed auth policy. Unknown factor/access/lockout modules fail closed.
use crate::{Error, protected_file};
use std::{io::Read, path::Path};
const PREAUTH: [&str; 2] = [
    "auth required pam_env.so",
    "auth required pam_faildelay.so delay=2000000",
];
fn auth_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            (fields
                .first()
                .is_some_and(|field| field.trim_start_matches('-') == "auth"))
            .then(|| fields.join(" "))
        })
        .collect()
}
fn password_policy(text: &str) -> bool {
    let lines = auth_lines(text);
    let mut required = PREAUTH.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if lines
        .get(2)
        .is_some_and(|s| s == "auth sufficient pam_fprintd.so")
    {
        required.push("auth sufficient pam_fprintd.so".into());
    }
    required.extend([
        "auth sufficient pam_unix.so nullok".into(),
        "auth required pam_deny.so".into(),
    ]);
    lines == required
}
fn read_policy(name: &str) -> Result<String, Error> {
    let override_path = Path::new("/etc/pam.d").join(name);
    let path = if override_path.exists() {
        override_path
    } else {
        Path::new("/usr/lib/pam.d").join(name)
    };
    // Fedora authselect creates administrator-owned links. Validate the resolved
    // root-owned path and its ancestors before reading any policy bytes.
    let path = path.canonicalize()?;
    let mut text = String::new();
    protected_file(&path, false, 16384)?
        .take(16385)
        .read_to_string(&mut text)?;
    if text.len() > 16384 {
        return Err("PAM policy exceeds size limit".into());
    }
    Ok(text)
}
pub fn check(service: Option<&str>) -> Result<(), Error> {
    let Some(service) = service else {
        return Ok(());
    };
    let password = if matches!(service, "sudo" | "sudo-i" | "login" | "hyprlock") {
        "system-auth"
    } else {
        "password-auth"
    };
    if !password_policy(&read_policy(password)?) {
        return Err(
            "Unsupported auth policy; use your password and request integration review".into(),
        );
    }
    let service_policy = read_policy(if service == "sudo-i" { "sudo" } else { service })?;
    let lines = auth_lines(&service_policy);
    // Activation checks use a private namespace and an explicit deny fallback.
    let diagnostic = format!(
        "auth sufficient {} diagnostic",
        "/usr/lib64/security/pam_linuxhello.so"
    );
    let face = format!(
        "auth sufficient {}",
        "/usr/lib64/security/pam_linuxhello.so"
    );
    let strict = vec![
        PREAUTH[0].into(),
        PREAUTH[1].into(),
        diagnostic,
        "auth required pam_deny.so".into(),
    ];
    if lines == strict {
        return Ok(());
    }
    if matches!(service, "sudo" | "sudo-i") {
        let expected = vec![
            PREAUTH[0].into(),
            PREAUTH[1].into(),
            face,
            "auth include system-auth".into(),
        ];
        if lines != expected {
            return Err("Unsupported sudo auth policy; reactivate the reviewed integration".into());
        }
    } else {
        let (companion_name, fallback) = match service {
            "login" => ("linuxhello-login-auth", "system-auth"),
            "hyprlock" => ("linuxhello-hyprlock-auth", "system-auth"),
            _ => ("linuxhello-kde-auth", "password-auth"),
        };
        let companion = auth_lines(&read_policy(companion_name)?);
        let expected = vec![
            PREAUTH[0].into(),
            PREAUTH[1].into(),
            face,
            format!("auth substack {fallback}"),
        ];
        let diagnostic_expected = vec![
            PREAUTH[0].into(),
            PREAUTH[1].into(),
            format!(
                "auth sufficient {} diagnostic",
                "/usr/lib64/security/pam_linuxhello.so"
            ),
            format!("auth substack {fallback}"),
        ];
        if !lines.contains(&format!("auth substack {companion_name}"))
            || (companion != expected && companion != diagnostic_expected)
        {
            return Err(
                "Unsupported desktop auth policy; reactivate the reviewed integration".into(),
            );
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_factor_lockout_access_and_nested_policy_are_refused() {
        let base = "auth required pam_env.so\nauth required pam_faildelay.so delay=2000000\nauth sufficient pam_unix.so nullok\nauth required pam_deny.so\n";
        assert!(password_policy(base));
        for extra in [
            "auth required pam_mfa.so\n",
            "auth required pam_faillock.so preauth\n",
            "auth include custom\n",
            "auth required pam_access.so\n",
        ] {
            assert!(!password_policy(&format!("{base}{extra}")));
            assert!(!password_policy(
                &base.replace("auth sufficient pam_unix.so nullok", extra.trim())
            ));
        }
        assert!(!password_policy(
            "auth sufficient pam_unix.so\nauth required pam_deny.so\n"
        ));
    }
}
