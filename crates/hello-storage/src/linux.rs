//! Linux-only secure filesystem boundary. Each unsafe block is a small libc call.

use super::{MAX_ENVELOPE_BYTES, SecretKey, StorageError, decrypt_template, encrypt_template};
use hello_face::Template;
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path},
};

const TEMPLATE: &CStr = c"template.lht";

pub struct SecureStore {
    directory: OwnedFd,
    owner_uid: u32,
    effective_uid: u32,
    key: SecretKey,
}

impl SecureStore {
    /// Open an existing absolute private directory without following any symlink.
    /// Root may manage a target user's store only after upstream authorization.
    /// A non-root process may open only its own UID namespace.
    pub fn open(path: &Path, owner_uid: u32, key: SecretKey) -> Result<Self, StorageError> {
        // SAFETY: geteuid takes no pointers and has no preconditions.
        let effective_uid = unsafe { libc::geteuid() };
        if effective_uid != 0 && owner_uid != effective_uid {
            return Err(StorageError::OwnerMismatch);
        }
        if !path.is_absolute() {
            return Err(StorageError::InsecurePath);
        }
        let mut directory = open_at(
            libc::AT_FDCWD,
            c"/",
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0,
        )?;
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    let name =
                        CString::new(name.as_bytes()).map_err(|_| StorageError::InsecurePath)?;
                    directory = open_at(
                        directory.as_raw_fd(),
                        &name,
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                        0,
                    )?;
                }
                _ => return Err(StorageError::InsecurePath),
            }
        }
        let metadata = File::from(directory.try_clone()?).metadata()?;
        if !metadata.is_dir() || metadata.uid() != effective_uid || metadata.mode() & 0o077 != 0 {
            return Err(StorageError::InsecurePath);
        }
        Ok(Self {
            directory,
            owner_uid,
            effective_uid,
            key,
        })
    }

    pub fn save(&self, template: &Template) -> Result<(), StorageError> {
        let envelope = encrypt_template(&self.key, self.owner_uid, template)?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| StorageError::RandomUnavailable)?;
        let name = format!(
            ".pending-{}",
            random
                .iter()
                .map(|value| format!("{value:02x}"))
                .collect::<String>()
        );
        let name = CString::new(name).map_err(|_| StorageError::InsecurePath)?;
        let descriptor = open_at(
            self.directory.as_raw_fd(),
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )?;
        let mut pending = Pending {
            directory: self.directory.as_raw_fd(),
            name: &name,
            remove: true,
        };
        let mut file = File::from(descriptor);
        file.write_all(&envelope)?;
        file.sync_all()?;
        // SAFETY: both descriptors stay alive; both C strings are NUL terminated.
        let result = unsafe {
            libc::renameat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                self.directory.as_raw_fd(),
                TEMPLATE.as_ptr(),
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        pending.remove = false;
        sync_directory(&self.directory)?;
        Ok(())
    }

    pub fn load(&self) -> Result<Option<Template>, StorageError> {
        let descriptor = match open_at(
            self.directory.as_raw_fd(),
            TEMPLATE,
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0,
        ) {
            Ok(descriptor) => descriptor,
            Err(StorageError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let file = File::from(descriptor);
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != self.effective_uid
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(StorageError::InsecurePath);
        }
        if metadata.len() > MAX_ENVELOPE_BYTES as u64 {
            return Err(StorageError::InvalidEnvelope);
        }
        let mut envelope = Vec::new();
        file.take(MAX_ENVELOPE_BYTES as u64 + 1)
            .read_to_end(&mut envelope)?;
        decrypt_template(&self.key, self.owner_uid, &envelope).map(Some)
    }

    /// Removes the directory entry and synchronizes it. This does not promise
    /// physical erasure on SSDs, snapshots, backups, or journaling filesystems.
    pub fn remove(&self) -> Result<bool, StorageError> {
        // SAFETY: owned directory descriptor and static NUL-terminated basename.
        let result = unsafe { libc::unlinkat(self.directory.as_raw_fd(), TEMPLATE.as_ptr(), 0) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                return Ok(false);
            }
            return Err(error.into());
        }
        sync_directory(&self.directory)?;
        Ok(true)
    }
}

fn open_at(
    directory: RawFd,
    path: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<OwnedFd, StorageError> {
    // SAFETY: CStr guarantees termination. mode is supplied even without O_CREAT.
    let descriptor = unsafe { libc::openat(directory, path.as_ptr(), flags, mode) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful openat returned a fresh owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
}

fn sync_directory(directory: &OwnedFd) -> Result<(), StorageError> {
    // SAFETY: the owned directory descriptor remains open for this call.
    if unsafe { libc::fsync(directory.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

struct Pending<'a> {
    directory: RawFd,
    name: &'a CStr,
    remove: bool,
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if self.remove {
            // SAFETY: guard never outlives save's directory or filename.
            unsafe { libc::unlinkat(self.directory, self.name.as_ptr(), 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn setup() -> (tempfile::TempDir, SecureStore) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        let store =
            SecureStore::open(directory.path(), uid, SecretKey::from_bytes([7; 32])).unwrap();
        (directory, store)
    }

    #[test]
    fn atomic_save_read_delete_and_permissions() {
        let (directory, store) = setup();
        assert!(store.load().unwrap().is_none());
        store.save(&crate::tests::template()).unwrap();
        assert_eq!(store.load().unwrap().unwrap().sample_count(), 3);
        let metadata = std::fs::metadata(directory.path().join("template.lht")).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        store.save(&crate::tests::template()).unwrap();
        assert!(store.remove().unwrap());
        assert!(!store.remove().unwrap());
        assert!(store.load().unwrap().is_none());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn symlink_and_hardlink_files_cannot_be_loaded() {
        let (directory, store) = setup();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let target = directory.path().join("template.lht");
        symlink(outside.path(), &target).unwrap();
        assert!(store.load().is_err());
        store.save(&crate::tests::template()).unwrap(); // rename replaces link, never follows it.
        assert_eq!(std::fs::metadata(outside.path()).unwrap().len(), 0);
        std::fs::hard_link(&target, directory.path().join("second-link")).unwrap();
        assert!(matches!(store.load(), Err(StorageError::InsecurePath)));
    }

    #[test]
    fn directory_alias_and_insecure_permissions_are_rejected() {
        let (directory, _store) = setup();
        let parent = tempfile::tempdir().unwrap();
        let alias = parent.path().join("alias");
        symlink(directory.path(), &alias).unwrap();
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        assert!(SecureStore::open(&alias, uid, SecretKey::from_bytes([7; 32])).is_err());
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            SecureStore::open(directory.path(), uid, SecretKey::from_bytes([7; 32])),
            Err(StorageError::InsecurePath)
        ));
    }

    #[test]
    fn tampered_file_never_returns_a_template() {
        let (directory, store) = setup();
        store.save(&crate::tests::template()).unwrap();
        let target = directory.path().join("template.lht");
        let mut bytes = std::fs::read(&target).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(target, bytes).unwrap();
        assert!(matches!(store.load(), Err(StorageError::InvalidEnvelope)));
    }
}
