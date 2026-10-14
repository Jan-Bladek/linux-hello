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
        let journal = dir.join(".profile-update");
        fs::create_dir(&journal)?;
        fs::set_permissions(&journal, fs::Permissions::from_mode(0o700))?;
        // Presence blocks readers even if preparing the journal is interrupted.
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
            version: 1,
            uid,
            template_before: before[0].1.is_some(),
            config_before: before[1].1.is_some(),
            key_before,
        };
        write_private(&journal.join("index.json"), &serde_json::to_vec(&index)?)?;
        std::fs::File::open(&journal)?.sync_all()?;
        std::fs::File::open(dir)?.sync_all()?;
        Ok(Self {
            dir: dir.to_path_buf(),
            before,
            key_before,
            committed: false,
        })
    }
    pub(super) fn commit(mut self) -> Result<(), Error> {
        remove_journal(&self.dir)?;
        self.committed = true;
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
fn remove_journal(dir: &Path) -> Result<(), Error> {
    let journal = dir.join(".profile-update");
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
pub(super) fn recover_profile(uid: u32) -> Result<(), Error> {
    if uid < 1000 {
        return Err("Normal account required".into());
    }
    let dir = directory(uid);
    let journal = dir.join(".profile-update");
    protected_path(&journal.join("index.json"))?;
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(&journal)?;
    if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o077 != 0 {
        return Err("Unsafe recovery journal".into());
    }
    let index: RecoveryIndex =
        serde_json::from_reader(protected_file(&journal.join("index.json"), true, 4096)?)?;
    if index.version != 1 || index.uid != uid {
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
