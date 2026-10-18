//! Fail-closed profile publication journal and explicit administrator recovery.
use crate::{Error, directory, protected_file, protected_path};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;
/// Durable interruption marker blocks authentication throughout publication.
/// Only encrypted template/configuration preimages are backed up on disk.
pub(super) struct ProfileWriteGuard {
    dir: PathBuf,
    before: Vec<(&'static str, Option<Zeroizing<Vec<u8>>>)>,
    committed: bool,
    key_before: bool,
    tpm_before: bool,
}
impl ProfileWriteGuard {
    pub(super) fn new(dir: &Path) -> Result<Self, Error> {
        let mut before = Vec::new();
        for name in ["template.lht", "config.json"] {
            let data = match protected_file(&dir.join(name), true, 70000) {
                Ok(mut f) => {
                    let mut data = Zeroizing::new(Vec::new());
                    f.read_to_end(&mut data)?;
                    Some(data)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            before.push((name, data));
        }
        let key_before = match protected_file(&dir.join("key"), true, 32) {
            Ok(file) => {
                if file.metadata()?.len() != 32 {
                    return Err("Invalid existing key length".into());
                }
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        let tpm_before = match protected_file(&dir.join("key.tpm"), true, 16384) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        match fs::symlink_metadata(dir.join(".key-tpm-pending")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err("Pending TPM key requires administrator recovery".into()),
        }
        // Prepare a complete durable index before publishing the blocking
        // marker. No active profile bytes change until new() returns.
        for name in [".profile-update", ".profile-retired"] {
            if fs::symlink_metadata(dir.join(name)).is_ok() {
                return Err("Profile recovery/cleanup required before publication".into());
            }
        }
        let journal = dir.join(".profile-prepare");
        fs::create_dir(&journal)?;
        fs::set_permissions(&journal, fs::Permissions::from_mode(0o700))?;
        std::fs::File::open(dir)?.sync_all()?;
        for (name, data) in &before {
            if let Some(data) = data {
                write_private(&journal.join(name), data)?;
            }
        }
        let uid = dir
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid profile UID")?
            .parse()?;
        let index = RecoveryIndex {
            version: 2,
            uid,
            template_before: before[0].1.is_some(),
            config_before: before[1].1.is_some(),
            key_before,
            tpm_before: Some(tpm_before),
        };
        write_private(&journal.join("index.json"), &serde_json::to_vec(&index)?)?;
        std::fs::File::open(&journal)?.sync_all()?;
        fs::rename(&journal, dir.join(".profile-update"))?;
        std::fs::File::open(dir)?.sync_all()?;
        Ok(Self {
            dir: dir.to_path_buf(),
            before,
            key_before,
            tpm_before,
            committed: false,
        })
    }
    pub(super) fn commit(mut self) -> Result<(), Error> {
        // Atomic retirement commits publication. Interrupted cleanup must never
        // look like an incomplete rollback journal or restore old bytes.
        retire_journal(&self.dir)?;
        self.committed = true;
        if cleanup_phase(&self.dir, ".profile-retired").is_err() {
            eprintln!("Linux Hello: profile committed; retired journal cleanup requires recover.");
        }
        Ok(())
    }
}
impl Drop for ProfileWriteGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut failed = false;
        for (name, data) in &self.before {
            let result = (|| -> Result<(), Error> {
                let target = self.dir.join(name);
                if let Some(data) = data {
                    let pending = self.dir.join(format!(".rollback-{name}"));
                    let mut f = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .custom_flags(libc::O_NOFOLLOW)
                        .open(&pending)?;
                    f.write_all(data)?;
                    f.sync_all()?;
                    fs::rename(pending, target)?;
                } else if target.exists() {
                    fs::remove_file(target)?;
                }
                Ok(())
            })();
            failed |= result.is_err();
        }
        if !self.key_before {
            match fs::remove_file(self.dir.join("key")) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => failed = true,
            }
        }
        if !self.tpm_before {
            failed |= remove_new_tpm(&self.dir).is_err();
        }
        let _ = fs::remove_file(self.dir.join(".config-pending"));
        if failed {
            // Removing config blocks all PAM services, including KDE markers.
            let _ = fs::remove_file(self.dir.join("config.json"));
            eprintln!(
                "Linux Hello: profile rollback failed; biometric configuration removed. Use your password."
            );
        }
        if !failed && remove_journal(&self.dir).is_err() {
            eprintln!("Linux Hello: recovery marker retained; administrator recovery required.");
        }
        if let Ok(dir) = std::fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryIndex {
    version: u32,
    uid: u32,
    template_before: bool,
    config_before: bool,
    key_before: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tpm_before: Option<bool>,
}
fn remove_new_tpm(dir: &Path) -> Result<(), Error> {
    for name in ["key.tpm", ".key-tpm-pending"] {
        match protected_file(&dir.join(name), true, 16384) {
            Ok(_) => fs::remove_file(dir.join(name))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub(super) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn retire_journal(dir: &Path) -> Result<(), Error> {
    if fs::symlink_metadata(dir.join(".profile-retired")).is_ok() {
        return Err("Retired journal already exists; administrator recovery required".into());
    }
    fs::rename(dir.join(".profile-update"), dir.join(".profile-retired"))?;
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}
fn cleanup_phase(dir: &Path, phase: &str) -> Result<(), Error> {
    let journal = dir.join(phase);
    protected_path(&journal.join("index.json"))?;
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(&journal)?;
    if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o077 != 0 {
        return Err("Unsafe profile journal phase".into());
    }
    for item in fs::read_dir(&journal)? {
        let item = item?;
        if !["template.lht", "config.json", "index.json"]
            .iter()
            .any(|name| item.file_name() == *name)
        {
            return Err("Unexpected profile journal entry".into());
        }
        protected_file(&item.path(), true, 70000)?;
    }
    for name in ["template.lht", "config.json", "index.json"] {
        fs::remove_file(journal.join(name)).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })?;
    }
    fs::remove_dir(journal)?;
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}
fn remove_journal(dir: &Path) -> Result<(), Error> {
    retire_journal(dir)?;
    cleanup_phase(dir, ".profile-retired")
}
pub(super) fn recover_profile(uid: u32) -> Result<(), Error> {
    if uid < 1000 {
        return Err("Normal account required".into());
    }
    let dir = directory(uid);
    let journal = dir.join(".profile-update");
    if fs::symlink_metadata(&journal).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        // These phases never need preimages: prepare precedes active writes;
        // retired follows durable commit/rollback. Cleanup is idempotent even
        // if index/backups were already partly removed by a previous cleanup.
        let mut cleaned = false;
        for phase in [".profile-prepare", ".profile-retired"] {
            if fs::symlink_metadata(dir.join(phase)).is_ok() {
                cleanup_phase(&dir, phase)?;
                cleaned = true;
            }
        }
        if cleaned {
            eprintln!("Profile journal cleanup completed; active reference retained.");
            return Ok(());
        }
    }
    protected_path(&journal.join("index.json"))?;
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(&journal)?;
    if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o077 != 0 {
        return Err("Unsafe recovery journal".into());
    }
    let index: RecoveryIndex =
        serde_json::from_reader(protected_file(&journal.join("index.json"), true, 4096)?)?;
    if !matches!((index.version, index.tpm_before), (1, None) | (2, Some(_))) || index.uid != uid {
        return Err("Recovery journal account/version mismatch".into());
    }
    let mut originals = Vec::new();
    for (name, present) in [
        ("template.lht", index.template_before),
        ("config.json", index.config_before),
    ] {
        let bytes = if present {
            let mut bytes = Zeroizing::new(Vec::new());
            protected_file(&journal.join(name), true, 70000)?.read_to_end(&mut bytes)?;
            Some(bytes)
        } else {
            None
        };
        originals.push((name, bytes));
    }
    // Validate every backup before replacing anything. The interruption marker
    // remains throughout recovery, so partial recovery cannot authenticate.
    for (name, bytes) in originals {
        let target = dir.join(name);
        if let Some(bytes) = bytes {
            let mut nonce = [0u8; 8];
            getrandom::fill(&mut nonce)?;
            let pending = dir.join(format!(".recovery-{:016x}", u64::from_le_bytes(nonce)));
            write_private(&pending, &bytes)?;
            fs::rename(pending, target)?;
        } else {
            fs::remove_file(target).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
        }
    }
    if !index.key_before {
        fs::remove_file(dir.join("key")).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })?;
    }
    if index.tpm_before == Some(false) {
        remove_new_tpm(&dir)?;
    }
    if fs::symlink_metadata(dir.join(".config-pending")).is_ok() {
        protected_file(&dir.join(".config-pending"), true, 16384)?;
        fs::remove_file(dir.join(".config-pending"))?;
    }
    std::fs::File::open(&dir)?.sync_all()?;
    remove_journal(&dir)?;
    eprintln!(
        "Previous protected reference restored. No PAM activation or camera capture performed."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "root private mount namespace with synthetic marker required"]
    fn interrupted_prepare_and_retired_cleanup_preserve_the_active_generation() {
        assert_eq!(
            fs::read(Path::new(crate::boundary::ROOT).join(".audit-test-only")).unwrap(),
            b"synthetic-only\n"
        );
        assert_ne!(
            fs::read_link("/proc/self/ns/mnt").unwrap(),
            fs::read_link("/proc/1/ns/mnt").unwrap()
        );
        // SAFETY: getuid takes no pointers and has no preconditions.
        assert_eq!(unsafe { libc::getuid() }, 0);
        let uid = 1002;
        let dir = directory(uid);
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        write_private(&dir.join("template.lht"), b"old-encrypted-fixture").unwrap();
        write_private(&dir.join("config.json"), b"old-config-fixture").unwrap();
        // Crash before index publication: active files were never touched.
        let preparing = dir.join(".profile-prepare");
        fs::create_dir(&preparing).unwrap();
        fs::set_permissions(&preparing, fs::Permissions::from_mode(0o700)).unwrap();
        write_private(&preparing.join("template.lht"), b"partial-backup").unwrap();
        assert!(ProfileWriteGuard::new(&dir).is_err());
        recover_profile(uid).unwrap();
        assert_eq!(
            fs::read(dir.join("template.lht")).unwrap(),
            b"old-encrypted-fixture"
        );
        assert!(!preparing.exists());
        // Crash after commit and during deletion: missing index is harmless
        // only in the atomically retired phase; never restore old preimages.
        let guard = ProfileWriteGuard::new(&dir).unwrap();
        fs::write(dir.join("template.lht"), b"new-encrypted-fixture").unwrap();
        fs::write(dir.join("config.json"), b"new-config-fixture").unwrap();
        retire_journal(&dir).unwrap();
        std::mem::forget(guard);
        fs::remove_file(dir.join(".profile-retired/index.json")).unwrap();
        recover_profile(uid).unwrap();
        assert_eq!(
            fs::read(dir.join("template.lht")).unwrap(),
            b"new-encrypted-fixture"
        );
        assert_eq!(
            fs::read(dir.join("config.json")).unwrap(),
            b"new-config-fixture"
        );
        assert!(!dir.join(".profile-retired").exists());
        let guard = ProfileWriteGuard::new(&dir).unwrap();
        guard.commit().unwrap();
        assert!(!dir.join(".profile-update").exists());
        // New v2 journals track fresh TPM publication without backing up any
        // plaintext key. Ordinary rollback and restart recovery remove only a
        // newly introduced wrapped provider; existing wrapped keys are retained.
        let guard = ProfileWriteGuard::new(&dir).unwrap();
        write_private(&dir.join("key.tpm"), b"synthetic-new-wrapped-key").unwrap();
        write_private(&dir.join(".key-tpm-pending"), b"synthetic-pending").unwrap();
        drop(guard);
        assert!(!dir.join("key.tpm").exists());
        assert!(!dir.join(".key-tpm-pending").exists());
        let guard = ProfileWriteGuard::new(&dir).unwrap();
        write_private(&dir.join("key.tpm"), b"synthetic-interrupted-wrapped-key").unwrap();
        std::mem::forget(guard);
        recover_profile(uid).unwrap();
        assert!(!dir.join("key.tpm").exists());
        write_private(&dir.join("key.tpm"), b"synthetic-existing-wrapped-key").unwrap();
        let guard = ProfileWriteGuard::new(&dir).unwrap();
        drop(guard);
        assert_eq!(
            fs::read(dir.join("key.tpm")).unwrap(),
            b"synthetic-existing-wrapped-key"
        );
        let _global_lease = crate::coordination::AttemptLease::acquire().unwrap();
        let reservation = crate::admission::Admission::reserve(uid).unwrap();
        assert!(crate::admission::Admission::reserve(uid).is_err());
        drop(reservation);
        assert!(crate::admission::Admission::reserve(uid).is_err());
    }
}
