//! Provisioning/read coordination, revocation epoch and startup budget.
use super::profile::write_private;
use crate::{Error, boundary, directory, protected_file, protected_path};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
};
use zeroize::Zeroizing;
pub(super) fn read_lock() -> Result<std::fs::File, Error> {
    let file = protected_file(&Path::new(boundary::ROOT).join(".setup.lock"), true, 1024)?;
    // SAFETY: file is a validated owned descriptor; shared locking cannot mutate data.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
        return Err("Account configuration is changing; use your password".into());
    }
    Ok(file)
}

pub(super) fn setup_lock() -> Result<std::fs::File, Error> {
    let path = Path::new(boundary::ROOT).join(".setup.lock");
    protected_path(&path)?;
    let lock = if let Some(raw) = std::env::var_os("LINUX_HELLO_SETUP_LOCK_FD") {
        let fd: i32 = raw.to_str().ok_or("Invalid lock descriptor")?.parse()?;
        // SAFETY: dup validates the descriptor; the duplicated descriptor has unique ownership.
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        use std::os::fd::FromRawFd;
        // SAFETY: the successful fcntl returned a new owned descriptor.
        unsafe { std::fs::File::from_raw_fd(duplicate) }
    } else {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?
    };
    use std::os::unix::fs::MetadataExt;
    let expected = protected_file(&path, true, 1024)?.metadata()?;
    let actual = lock.metadata()?;
    if expected.ino() != actual.ino() || expected.dev() != actual.dev() {
        return Err("Invalid setup lock".into());
    }
    // SAFETY: lock is an owned, validated file descriptor.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Another setup operation is active".into());
    }
    Ok(lock)
}

pub(super) struct AttemptLease(std::fs::File);
impl AttemptLease {
    pub(super) fn acquire() -> Result<Self, Error> {
        // Serialize expensive root TPM/model/camera startup. Contention falls back
        // immediately to the password; no queue can grow without a bound.
        protected_file(&Path::new(boundary::ROOT).join(".anchor"), true, 1024)?;
        let file = std::fs::File::open(boundary::ROOT)?;
        // SAFETY: valid owned descriptor, integer-only flock operation.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Face verification busy; use password authentication".into());
        }
        Ok(Self(file))
    }
}
impl Drop for AttemptLease {
    fn drop(&mut self) {
        let _ = self.0.metadata();
    } // owning the FD holds the lease until drop
}
pub(super) fn activation_allowed(uid: u32, service: Option<&str>) -> Result<(), Error> {
    match service {
        None => Ok(()), // explicit root diagnostic/enrollment, never selected by unprivileged PAM
        Some("sudo" | "sudo-i") => {
            match fs::symlink_metadata(directory(uid).join("pam-sudo.disabled")) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                _ => Err("Face authentication disabled".into()),
            }
        }
        Some(service @ ("kde" | "plasmalogin" | "sddm")) => {
            let mut bytes = Vec::new();
            protected_file(
                &directory(uid).join(format!("pam-{service}.enabled")),
                true,
                64,
            )?
            .read_to_end(&mut bytes)?;
            if bytes != b"linux-hello-login-v1\n" {
                return Err("Face authentication disabled".into());
            }
            Ok(())
        }
        _ => Err("Unsupported authentication service".into()),
    }
}
pub(super) fn bump_epoch() -> Result<(), Error> {
    let root = Path::new(boundary::ROOT);
    let path = root.join(".auth-epoch");
    protected_path(&path)?;
    if fs::symlink_metadata(&path).is_ok() {
        protected_file(&path, true, 32)?;
    }
    let mut random = [0; 32];
    getrandom::fill(&mut random)?;
    let mut nonce = [0; 8];
    getrandom::fill(&mut nonce)?;
    let pending = root.join(format!(".epoch-{:016x}", u64::from_le_bytes(nonce)));
    write_private(&pending, &random)?;
    fs::rename(pending, path)?;
    std::fs::File::open(root)?.sync_all()?;
    Ok(())
}
pub(super) fn profile_snapshot(uid: u32) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut result = Zeroizing::new(Vec::with_capacity(110_000));
    match protected_file(&Path::new(boundary::ROOT).join(".auth-epoch"), true, 32) {
        Ok(mut epoch) => {
            if epoch.metadata()?.len() != 32 {
                return Err("Invalid revocation epoch".into());
            }
            epoch.read_to_end(&mut result)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    for (name, maximum) in [
        ("config.json", 16384),
        ("template.lht", 70000),
        ("key", 32),
        ("key.tpm", 16384),
    ] {
        let path = directory(uid).join(name);
        match protected_file(&path, true, maximum) {
            Ok(mut file) => {
                result.extend_from_slice(name.as_bytes());
                result.extend_from_slice(&file.metadata()?.len().to_le_bytes());
                file.read_to_end(&mut result)?;
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && matches!(name, "key" | "key.tpm") => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(result)
}
