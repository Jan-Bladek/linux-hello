//! Opt-in sudo-only PAM adapter. Native inference never runs inside sudo.
use crate::boundary;
#[cfg(feature = "kde-pam")]
use std::io::Read;
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const PAM_SUCCESS: c_int = 0;
const PAM_IGNORE: c_int = 25;
// Non-secret transaction marker. It contains no token, password or biometric.
static MATCHED: u8 = 1;
const MATCH_DATA: &CStr = c"linux-hello-face-matched-v1";
#[link(name = "libpam.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn pam_set_data(
        pamh: *mut c_void,
        name: *const c_char,
        data: *mut c_void,
        cleanup: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, c_int)>,
    ) -> c_int;
    fn pam_get_data(pamh: *const c_void, name: *const c_char, data: *mut *const c_void) -> c_int;
    fn pam_get_item(pamh: *const c_void, item: c_int, out: *mut *const c_void) -> c_int;
    fn pam_get_user(pamh: *mut c_void, out: *mut *const c_char, prompt: *const c_char) -> c_int;
    fn pam_prompt(
        pamh: *mut c_void,
        style: c_int,
        response: *mut *mut c_char,
        format: *const c_char,
        ...
    ) -> c_int;
}
fn permitted(service: &str, uid: u32, caller: u32) -> bool {
    if uid < 1000 {
        return false;
    }
    match service {
        "sudo" | "sudo-i" => uid == caller,
        #[cfg(feature = "kde-pam")]
        // A root display-manager helper supplies the selected PAM_USER. There
        // is no logged-in caller and no PAM_RUSER to use before login.
        "plasmalogin" | "sddm" => caller == 0,
        #[cfg(feature = "kde-pam")]
        "kde" => caller == uid,
        _ => false,
    }
}
fn diagnostic_allowed(service: &str, caller: u32, requested: bool) -> bool {
    cfg!(feature = "kde-pam")
        && requested
        && caller == 0
        && matches!(service, "plasmalogin" | "sddm")
}
fn authenticate(pamh: *mut c_void, silent: bool, diagnostic: bool) -> Option<()> {
    if pamh.is_null() {
        return None;
    }
    let mut service = std::ptr::null();
    let mut user = std::ptr::null();
    // SAFETY: PAM supplied a live handle; outputs are writable. PAM owns the
    // returned terminated strings for the duration of this invocation.
    if unsafe { pam_get_item(pamh, 1, &mut service) } != PAM_SUCCESS || service.is_null() {
        return None;
    }
    // SAFETY: same live handle; null prompt means use standard PAM_USER prompt.
    if unsafe { pam_get_user(pamh, &mut user, std::ptr::null()) } != PAM_SUCCESS || user.is_null() {
        return None;
    }
    // SAFETY: successful PAM getters returned terminated strings.
    let service = unsafe { CStr::from_ptr(service.cast()) }.to_str().ok()?;
    // SAFETY: pam_get_user returned a valid string owned by this PAM transaction.
    let user = unsafe { CStr::from_ptr(user) }.to_str().ok()?;
    let (uid, _, _) = boundary::account(user).ok()?;
    // sudo may switch all process UIDs to root before PAM. In that case use
    // PAM_RUSER set by the trusted sudo plugin, never SUDO_USER/environment.
    // SAFETY: getuid has no preconditions.
    let mut caller = unsafe { libc::getuid() };
    if caller == 0 && matches!(service, "sudo" | "sudo-i") {
        let mut ruser = std::ptr::null();
        // SAFETY: live PAM handle and writable output, item 8 is PAM_RUSER.
        if unsafe { pam_get_item(pamh, 8, &mut ruser) } != PAM_SUCCESS || ruser.is_null() {
            return None;
        }
        // SAFETY: successful getter returned a terminated PAM-owned string.
        let name = unsafe { CStr::from_ptr(ruser.cast()) }.to_str().ok()?;
        caller = boundary::account(name).ok()?.0;
    }
    if !permitted(service, uid, caller) {
        return None;
    }
    // SAFETY: geteuid takes no pointers. Only the self-bound locker launcher
    // accepts a non-root PAM caller; sudo/display-manager paths remain root.
    if unsafe { libc::geteuid() } != 0 && service != "kde" {
        return None;
    }
    if service == "kde" {
        boundary::protected_file(Path::new(boundary::LOCK_HELPER), false, 10_000_000).ok()?;
        let child = Command::new(boundary::LOCK_HELPER)
            .env_clear()
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        return wait_helper(child, Duration::from_secs(25), Duration::from_secs(10));
    }
    #[cfg(feature = "kde-pam")]
    if matches!(service, "plasmalogin" | "sddm") {
        // Independent, root-only, per-account opt-in. Merely enabling sudo or
        // loading this module in a greeter/autologin stack cannot enable login.
        let marker = Path::new(boundary::ROOT)
            .join(uid.to_string())
            .join(format!("pam-{service}.enabled"));
        let mut bytes = Vec::new();
        boundary::protected_file(&marker, true, 64)
            .ok()?
            .take(65)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes != b"linux-hello-login-v1\n" {
            return None;
        }
    }
    boundary::protected_file(Path::new(boundary::HELPER), false, 100_000_000).ok()?;
    // Only the configured UID can enable this path. Missing enrollment falls
    // through immediately, without opening the camera or suppressing passwords.
    boundary::protected_file(
        &Path::new(boundary::ROOT)
            .join(uid.to_string())
            .join("config.json"),
        true,
        16384,
    )
    .ok()?;
    if !silent {
        // SAFETY: valid handle, informational message (no response required),
        // constant format and a terminated string matching the single %s.
        unsafe {
            pam_prompt(
                pamh,
                4,
                std::ptr::null_mut(),
                c"%s".as_ptr(),
                c"Linux Hello: spójrz prosto w kamerę. W razie braku dopasowania użyj hasła."
                    .as_ptr(),
            )
        };
    }
    // Explicit administrator PAM option, used only in the private activation
    // test stack. Same verifier and exit-status checks; never an auth bypass.
    let diagnostic = diagnostic_allowed(service, caller, diagnostic);
    let child = Command::new(boundary::HELPER)
        .args([
            if diagnostic { "diagnose" } else { "verify" },
            &uid.to_string(),
        ])
        .env_clear()
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if diagnostic {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .spawn()
        .ok()?;
    wait_helper(child, Duration::from_secs(25), Duration::from_secs(10))
}
fn wait_helper(mut child: std::process::Child, timeout: Duration, grace: Duration) -> Option<()> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return (status.success() && started.elapsed() < timeout).then_some(());
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => (),
        }
        if started.elapsed() >= timeout {
            // SAFETY: child is still owned/unreaped and its positive PID is valid.
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            let cleanup = Instant::now();
            while cleanup.elapsed() < grace {
                if !matches!(child.try_wait(), Ok(None)) {
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
/// # Safety
/// The PAM caller must supply a live handle and valid argc/argv module options,
/// as required by Linux-PAM.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_authenticate(
    pamh: *mut c_void,
    flags: c_int,
    argc: c_int,
    argv: *const *const c_char,
) -> c_int {
    match std::panic::catch_unwind(|| {
        if pamh.is_null() {
            return None;
        }
        // SAFETY: PAM supplies a live handle. Reset every attempt, including
        // retries on the same handle, so a prior match cannot outlive a failure.
        if unsafe { pam_set_data(pamh, MATCH_DATA.as_ptr(), std::ptr::null_mut(), None) }
            != PAM_SUCCESS
        {
            return None;
        }
        let diagnostic = if argc == 1 && !argv.is_null() {
            // SAFETY: PAM supplies argc valid pointers to terminated options.
            let option = unsafe { *argv };
            // SAFETY: non-null option is a terminated string provided by PAM.
            !option.is_null() && unsafe { CStr::from_ptr(option) } == c"diagnostic"
        } else {
            false
        };
        authenticate(pamh, flags & 0x8000 != 0, diagnostic)?;
        let marker = std::ptr::from_ref(&MATCHED).cast_mut().cast();
        // SAFETY: marker points at immutable static storage, never dereferenced
        // or freed by PAM. No cleanup is needed; the handle owns only its entry.
        (unsafe { pam_set_data(pamh, MATCH_DATA.as_ptr(), marker, None) } == PAM_SUCCESS)
            .then_some(())
    }) {
        Ok(Some(())) => PAM_SUCCESS,
        _ => PAM_IGNORE,
    }
}
/// # Safety
/// Called with a live PAM handle; this adapter has no extra credentials to
/// create. A no-op success follows only its own match on this transaction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_setcred(
    pamh: *mut c_void,
    _flags: c_int,
    _argc: c_int,
    _argv: *const *const c_char,
) -> c_int {
    if pamh.is_null() {
        return PAM_IGNORE;
    }
    let mut data = std::ptr::null();
    // SAFETY: the PAM caller supplies the live handle and output is writable;
    // data is only compared as an address, never dereferenced.
    let status = unsafe { pam_get_data(pamh, MATCH_DATA.as_ptr(), &mut data) };
    credential_status(status, data)
}
fn credential_status(status: c_int, data: *const c_void) -> c_int {
    if status == PAM_SUCCESS && data == std::ptr::from_ref(&MATCHED).cast() {
        PAM_SUCCESS
    } else {
        PAM_IGNORE
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_succeed_only_for_this_transactions_match() {
        assert_eq!(
            credential_status(PAM_SUCCESS, std::ptr::from_ref(&MATCHED).cast()),
            PAM_SUCCESS
        );
        assert_eq!(credential_status(PAM_SUCCESS, std::ptr::null()), PAM_IGNORE);
        let unrelated = 1_u8;
        assert_eq!(
            credential_status(PAM_SUCCESS, std::ptr::from_ref(&unrelated).cast()),
            PAM_IGNORE
        );
        assert_eq!(
            credential_status(18, std::ptr::from_ref(&MATCHED).cast()),
            PAM_IGNORE
        );
        // SAFETY: null is explicitly checked by the export before calling PAM.
        let status = unsafe { pam_sm_setcred(std::ptr::null_mut(), 2, 0, std::ptr::null()) };
        assert_eq!(status, PAM_IGNORE);
    }
    #[test]
    fn child_failure_and_timeouts_never_become_success() {
        let spawn = |program: &str, args: &[&str]| {
            Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        };
        assert!(
            wait_helper(
                spawn("/usr/bin/true", &[]),
                Duration::from_secs(2),
                Duration::from_millis(100)
            )
            .is_some()
        );
        assert!(
            wait_helper(
                spawn("/usr/bin/false", &[]),
                Duration::from_secs(2),
                Duration::from_millis(100)
            )
            .is_none()
        );
        // Even an already successful child cannot authenticate after its deadline.
        let mut exited = spawn("/usr/bin/true", &[]);
        exited.wait().unwrap();
        assert!(wait_helper(exited, Duration::ZERO, Duration::ZERO).is_none());
        // Cancellation may make a helper exit0; the expired parent still rejects.
        let cancelled = spawn(
            "/usr/bin/python3",
            &[
                "-c",
                "import signal,time; signal.signal(signal.SIGTERM, lambda *x: exit(0)); time.sleep(5)",
            ],
        );
        assert!(
            wait_helper(
                cancelled,
                Duration::from_millis(150),
                Duration::from_millis(150)
            )
            .is_none()
        );
        let hung = spawn(
            "/usr/bin/python3",
            &[
                "-c",
                "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(5)",
            ],
        );
        let pid = hung.id();
        assert!(wait_helper(hung, Duration::from_millis(150), Duration::from_millis(50)).is_none());
        // SAFETY: kill with signal0 checks existence without delivering a signal.
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }
    #[test]
    fn diagnostics_are_only_for_explicit_root_login_tests() {
        for service in ["plasmalogin", "sddm"] {
            assert_eq!(
                diagnostic_allowed(service, 0, true),
                cfg!(feature = "kde-pam")
            );
            assert!(!diagnostic_allowed(service, 0, false));
            assert!(!diagnostic_allowed(service, 1000, true));
        }
        for service in ["sudo", "sudo-i", "kde", "plasmalogin-autologin", "sshd"] {
            assert!(!diagnostic_allowed(service, 0, true));
        }
    }
    #[test]
    fn restricts_service_and_claimed_identity() {
        assert!(permitted("sudo", 1000, 1000));
        assert!(permitted("sudo-i", 1000, 1000));
        for service in ["login", "sshd", "su", "sudo-test", ""] {
            assert!(!permitted(service, 1000, 1000));
        }
        assert!(!permitted("sudo", 0, 0));
        assert!(!permitted("sudo", 1001, 1000));
        assert!(authenticate(std::ptr::null_mut(), true, false).is_none());
    }
    #[test]
    fn graphical_login_requires_a_root_greeter_and_normal_claimed_account() {
        for service in ["plasmalogin", "sddm"] {
            assert_eq!(permitted(service, 1000, 0), cfg!(feature = "kde-pam"));
            assert!(!permitted(service, 1000, 1000));
            assert!(!permitted(service, 0, 0));
        }
        for service in [
            "plasmalogin-greeter",
            "plasmalogin-autologin",
            "sddm-greeter",
            "sddm-autologin",
            "kde-fingerprint",
            "login",
            "sshd",
        ] {
            assert!(!permitted(service, 1000, 0));
        }
    }
    #[test]
    fn locker_only_verifies_its_real_callers_account() {
        assert_eq!(permitted("kde", 1000, 1000), cfg!(feature = "kde-pam"));
        assert!(!permitted("kde", 1001, 1000));
        assert!(!permitted("kde", 1000, 0));
        assert!(!permitted("kde", 0, 0));
    }
}
