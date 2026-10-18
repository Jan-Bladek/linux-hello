//! Optional TPM-only wrapping of the existing per-account template key.
//! No PCR binding: protects copied disks, not a compromised running host.
use super::{Error, directory};
#[cfg(test)]
use hello_pam::boundary::ROOT;
use hello_pam::boundary::protected_file;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
const TOOL: &str = "/usr/bin/systemd-creds";
const MAX_BLOB: u64 = 16384;
const TIMEOUT: Duration = Duration::from_secs(8);

struct Supervised(Child);
impl Drop for Supervised {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn name(uid: u32) -> Result<String, Error> {
    if uid < 1000 {
        return Err("TPM profile requires a normal account UID".into());
    }
    Ok(format!("linux-hello-key-v1-{uid}"))
}
// Inputs are either a protected ciphertext FD or the 32-byte in-memory key.
// Output is bounded, zeroized, and never forwarded to logs or a terminal.
fn execute(uid: u32, input: Stdio, secret: Option<&[u8; 32]>) -> Result<Zeroizing<Vec<u8>>, Error> {
    protected_file(Path::new(TOOL), false, 10_000_000)?;
    let mut command = Command::new(TOOL);
    command.env_clear().env("LANG", "C.UTF-8").current_dir("/");
    command.arg(format!("--name={}", name(uid)?));
    // SAFETY: pre_exec performs only async-signal-safe libc calls. The parent
    // race is closed before exec, and no borrowed pointers cross the fork.
    unsafe {
        command.pre_exec(|| {
            let parent = libc::getppid();
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if parent == 1 || libc::getppid() != parent {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            Ok(())
        });
    }
    if secret.is_some() {
        command.args([
            "encrypt",
            "--with-key=tpm2",
            "--tpm2-device=/dev/tpmrm0",
            "--tpm2-pcrs=",
            "--tpm2-public-key-pcrs=",
            "-",
            "-",
        ]);
    } else {
        command.args([
            "decrypt",
            "--tpm2-device=/dev/tpmrm0",
            "--refuse-null",
            "--newline=no",
            "-",
            "-",
        ]);
    }
    let mut child = Supervised(
        command
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    if let Some(secret) = secret {
        child
            .0
            .stdin
            .take()
            .ok_or("Missing TPM input pipe")?
            .write_all(secret)?;
    }
    let mut stdout = child.0.stdout.take().ok_or("Missing TPM output pipe")?;
    // SAFETY: F_GETFL/F_SETFL operate on the live stdout FD without pointers.
    let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
    // SAFETY: the descriptor is valid; preserve existing flags and add nonblocking.
    let nonblocking =
        unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if flags < 0 || nonblocking < 0 {
        return Err("Cannot supervise TPM output".into());
    }
    let start = Instant::now();
    let limit = if secret.is_some() {
        MAX_BLOB as usize
    } else {
        32
    };
    // Reserve once so secret output cannot leave a freed allocation on growth.
    let mut output = Zeroizing::new(Vec::with_capacity(limit));
    let mut buffer = Zeroizing::new([0; 1024]);
    loop {
        if start.elapsed() >= TIMEOUT {
            return Err("TPM operation timed out; use password authentication".into());
        }
        match stdout.read(&mut *buffer) {
            Ok(0) => break,
            Ok(size) => {
                if output.len() + size > limit {
                    return Err("Invalid TPM response length".into());
                }
                output.extend_from_slice(&buffer[..size]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
            Err(error) => return Err(error.into()),
        }
    }
    loop {
        if let Some(status) = child.0.try_wait()? {
            if !status.success() || output.is_empty() {
                return Err("TPM key operation failed; use password authentication".into());
            }
            return Ok(output);
        }
        if start.elapsed() >= TIMEOUT {
            return Err("TPM operation timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn unseal(uid: u32, file: File) -> Result<hello_storage::SecretKey, Error> {
    let output = execute(uid, Stdio::from(file), None)?;
    decode_key(&output)
}
fn decode_key(output: &[u8]) -> Result<hello_storage::SecretKey, Error> {
    let bytes: [u8; 32] = output
        .try_into()
        .map_err(|_| "Invalid unsealed key length")?;
    let bytes = Zeroizing::new(bytes);
    Ok(hello_storage::SecretKey::from_bytes(*bytes))
}
/// Presence of a credential selects TPM; errors never fall back to plaintext.
pub(super) fn load(uid: u32) -> Result<Option<hello_storage::SecretKey>, Error> {
    let path = directory(uid).join("key.tpm");
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(unseal(uid, protected_file(&path, true, MAX_BLOB)?)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
/// New profiles require independent TPM wrapping. Never create a plaintext key
/// and never fall back if sealing or unsealing fails. Existing legacy profiles
/// remain readable until an administrator explicitly migrates them.
pub(super) fn create_key(uid: u32) -> Result<hello_storage::SecretKey, Error> {
    name(uid)?;
    for name in ["key", "key.tpm", ".key-tpm-pending"] {
        match fs::symlink_metadata(directory(uid).join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => {
                return Err(
                    "Existing key provider or pending TPM operation requires review".into(),
                );
            }
        }
    }
    let mut secret = Zeroizing::new([0; 32]);
    getrandom::fill(&mut *secret)?;
    let blob = execute(uid, Stdio::piped(), Some(&secret))?;
    persist_new_key(&directory(uid), &blob, |pending| {
        unseal(uid, protected_file(pending, true, MAX_BLOB)?)
    })
}
fn persist_new_key(
    dir: &Path,
    blob: &[u8],
    recover: impl FnOnce(&Path) -> Result<hello_storage::SecretKey, Error>,
) -> Result<hello_storage::SecretKey, Error> {
    for name in ["key", "key.tpm", ".key-tpm-pending"] {
        match fs::symlink_metadata(dir.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => {
                return Err(
                    "Existing key provider or pending TPM operation requires review".into(),
                );
            }
        }
    }
    let pending = dir.join(".key-tpm-pending");
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&pending)?;
        file.write_all(blob)?;
        file.sync_all()?;
        let recovered = recover(&pending)?;
        fs::rename(&pending, dir.join("key.tpm"))?;
        File::open(dir)?.sync_all()?;
        Ok(recovered)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result
}

/// Explicit root migration; verify full template before removing the old key.
/// Shares the GUI setup lock. Never changes PAM configuration or camera policy.
pub(super) fn enable(uid: u32) -> Result<(), Error> {
    name(uid)?;
    let path = directory(uid).join("key.tpm");
    if fs::symlink_metadata(&path).is_ok() {
        let (mut config, template) = super::load(uid)?; // validate authenticated full template
        drop(template);
        let guard = super::profile::ProfileWriteGuard::new(&directory(uid))?;
        config.key_provider = Some(super::KeyProvider::Tpm);
        publish_provider(uid, &config)?;
        guard.commit()?;
        remove_plaintext(uid)?;
        return Ok(());
    }
    // Fails if the existing template/config/key is missing or corrupt.
    let (mut config, template) = super::load(uid)?;
    drop(template);
    let guard = super::profile::ProfileWriteGuard::new(&directory(uid))?;
    let mut secret = Zeroizing::new([0; 32]);
    protected_file(&directory(uid).join("key"), true, 32)?.read_exact(&mut *secret)?;
    let blob = execute(uid, Stdio::piped(), Some(&secret))?;
    let pending = directory(uid).join(".key-tpm-pending");
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&pending)?;
    // Only ciphertext ever reaches a file. Stale pending files fail closed.
    let result = (|| -> Result<(), Error> {
        file.write_all(&blob)?;
        file.sync_all()?;
        let recovered = unseal(uid, protected_file(&pending, true, MAX_BLOB)?)?;
        let store = hello_storage::SecureStore::open(&directory(uid), uid, recovered)?;
        store
            .load()?
            .ok_or("No authenticated template after TPM sealing")?;
        fs::rename(&pending, &path)?;
        File::open(directory(uid))?.sync_all()?;
        config.key_provider = Some(super::KeyProvider::Tpm);
        publish_provider(uid, &config)?;
        guard.commit()?;
        remove_plaintext(uid)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result
}
fn publish_provider(uid: u32, config: &super::Config) -> Result<(), Error> {
    let pending = directory(uid).join(".config-pending");
    super::profile::write_private(&pending, &serde_json::to_vec(config)?)?;
    fs::rename(pending, directory(uid).join("config.json"))?;
    File::open(directory(uid))?.sync_all()?;
    Ok(())
}
fn remove_plaintext(uid: u32) -> Result<(), Error> {
    let path = directory(uid).join("key");
    match protected_file(&path, true, 32) {
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    File::open(directory(uid))?.sync_all()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_provider_never_creates_plaintext_and_requires_successful_recovery() {
        assert!(create_key(999).is_err());
        let dir = tempfile::tempdir().unwrap();
        let result = persist_new_key(dir.path(), b"synthetic-wrapped-key", |_| {
            Err("simulated unseal failure".into())
        });
        assert!(result.is_err());
        assert!(!dir.path().join("key").exists());
        assert!(!dir.path().join("key.tpm").exists());
        assert!(!dir.path().join(".key-tpm-pending").exists());
        let _key = persist_new_key(dir.path(), b"synthetic-wrapped-key", |_| {
            Ok(hello_storage::SecretKey::from_bytes([7; 32]))
        })
        .unwrap();
        assert_eq!(
            fs::read(dir.path().join("key.tpm")).unwrap(),
            b"synthetic-wrapped-key"
        );
        assert!(!dir.path().join("key").exists());
        assert!(
            persist_new_key(dir.path(), b"replacement", |_| panic!(
                "must not replace an existing provider"
            ))
            .is_err()
        );
    }

    #[test]
    fn exact_binary_key_and_account_binding() {
        assert!(decode_key(&[0; 32]).is_ok());
        for length in [0, 31, 33, 1024] {
            assert!(decode_key(&vec![0; length]).is_err());
        }
        assert!(name(0).is_err());
        assert!(name(999).is_err());
        assert_ne!(name(1000).unwrap(), name(1001).unwrap());
        assert_eq!(name(1000).unwrap(), "linux-hello-key-v1-1000");
    }
    #[test]
    #[ignore = "real TPM and root in an isolated mount namespace required"]
    fn real_tpm_migration_and_fail_closed() {
        // Never run against production profiles, even when invoked by root.
        // SAFETY: geteuid has no pointer arguments or preconditions.
        assert_eq!(unsafe { libc::geteuid() }, 0);
        assert_ne!(
            fs::read_link("/proc/self/ns/mnt").unwrap(),
            fs::read_link("/proc/1/ns/mnt").unwrap()
        );
        assert_eq!(
            fs::read(Path::new(ROOT).join(".tpm-test-only")).unwrap(),
            b"isolated-synthetic-fixture\n"
        );
        assert!(!directory(1000).exists());
        fs::create_dir(directory(1000)).unwrap();
        use hello_face::{Embedding, Enrollment, QualityMetrics};
        let mut enrollment = Enrollment::new(0.8).unwrap();
        for index in 0..3 {
            enrollment
                .add(
                    [index; 32],
                    Embedding::new([9; 32], vec![1.0, 0.05 * f32::from(index)]).unwrap(),
                    1,
                    QualityMetrics {
                        mean_intensity: 0.5,
                        contrast: 0.1,
                        edge_energy: 0.1,
                    },
                )
                .unwrap();
        }
        let template = enrollment.finish().unwrap();
        let mut secret = Zeroizing::new([0; 32]);
        getrandom::fill(&mut *secret).unwrap();
        let mut plain = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory(1000).join("key"))
            .unwrap();
        plain.write_all(&*secret).unwrap();
        let key = hello_storage::SecretKey::from_bytes(*secret);
        hello_storage::SecureStore::open(&directory(1000), 1000, key)
            .unwrap()
            .save(&template)
            .unwrap();
        let config = super::super::Config {
            version: 1,
            uid: 1000,
            device: "/dev/video2".into(),
            identity: Default::default(),
            threshold: 0.93,
            experimental_without_validated_pad: true,
            key_provider: None,
        };
        let mut cfg = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory(1000).join("config.json"))
            .unwrap();
        cfg.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        let before = fs::read(directory(1000).join("template.lht")).unwrap();
        fs::write(directory(1000).join("template.lht"), b"corrupt").unwrap();
        assert!(enable(1000).is_err());
        assert!(directory(1000).join("key").exists());
        assert!(!directory(1000).join("key.tpm").exists());
        fs::write(directory(1000).join("template.lht"), &before).unwrap();
        enable(1000).unwrap();
        assert!(!directory(1000).join("key").exists());
        assert!(load(1000).unwrap().is_some());
        assert_eq!(super::super::load(1000).unwrap().0.threshold, 0.93);
        assert_eq!(
            fs::read(directory(1000).join("template.lht")).unwrap(),
            before
        );
        enable(1000).unwrap(); // restart/idempotent cleanup uses sealed key
        let sealed = fs::read(directory(1000).join("key.tpm")).unwrap();
        fs::create_dir(directory(1001)).unwrap();
        let mut other = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory(1001).join("key.tpm"))
            .unwrap();
        other.write_all(&sealed).unwrap();
        assert!(load(1001).is_err()); // different authenticated credential name
        // Corrupt credential must reject even if an old plaintext key exists.
        plain = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory(1000).join("key"))
            .unwrap();
        plain.write_all(&*secret).unwrap();
        // A missing TPM must reject, even with the old plaintext key present.
        let fake_device = Path::new(ROOT).join(".fake-tpm");
        fs::write(&fake_device, []).unwrap();
        assert!(
            Command::new("/usr/bin/mount")
                .args(["--bind"])
                .arg(&fake_device)
                .arg("/dev/tpmrm0")
                .status()
                .unwrap()
                .success()
        );
        let rejected = super::super::key(1000).is_err();
        assert!(
            Command::new("/usr/bin/umount")
                .arg("/dev/tpmrm0")
                .status()
                .unwrap()
                .success()
        );
        assert!(rejected);
        // A stalled credential process is killed/reaped on the bounded deadline.
        let fake_tool = Path::new(ROOT).join(".fake-creds");
        let mut fake = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&fake_tool)
            .unwrap();
        fake.write_all(b"#!/bin/sh\nexec /usr/bin/sleep 30\n")
            .unwrap();
        drop(fake);
        assert!(
            Command::new("/usr/bin/mount")
                .arg("--bind")
                .arg(&fake_tool)
                .arg(TOOL)
                .status()
                .unwrap()
                .success()
        );
        let start = Instant::now();
        let rejected = super::super::key(1000).is_err();
        let elapsed = start.elapsed();
        assert!(
            Command::new("/usr/bin/umount")
                .arg(TOOL)
                .status()
                .unwrap()
                .success()
        );
        assert!(rejected);
        assert!(elapsed < Duration::from_secs(10));
        fs::write(directory(1000).join("key.tpm"), b"corrupt").unwrap();
        assert!(super::super::key(1000).is_err());
        fs::remove_file(directory(1000).join("key.tpm")).unwrap();
        std::os::unix::fs::symlink("missing", directory(1000).join("key.tpm")).unwrap();
        assert!(super::super::key(1000).is_err());
        fs::remove_file(directory(1000).join("key.tpm")).unwrap();
        let mut restored = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory(1000).join("key.tpm"))
            .unwrap();
        restored.write_all(&sealed).unwrap();
        fs::remove_file(directory(1000).join("key")).unwrap();
    }
}
