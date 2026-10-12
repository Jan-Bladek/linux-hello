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
    /// Explicit administrator-authorized import. Reads an existing user-owned
    /// encrypted store only; never creates or writes files under that identity.
    /// The caller must authorize the target UID independently (fresh sudo).
    pub fn import_passphrase(
        path: &Path,
        owner_uid: u32,
        password: &[u8],
    ) -> Result<Template, StorageError> {
        // SAFETY: geteuid takes no pointers.
        if unsafe { libc::geteuid() } != 0 || owner_uid < 1000 {
            return Err(StorageError::OwnerMismatch);
        }
        let directory = open_directory(path, false, owner_uid)?;
        lock_directory(&directory)?;
        let salt = read_or_create_salt(&directory, owner_uid, false)?;
        let key = super::passphrase_key(password, &salt)?;
        let store = Self {
            directory,
            owner_uid,
            effective_uid: owner_uid,
            key,
        };
        store.load()?.ok_or(StorageError::InvalidEnvelope)
    }
    /// Open an existing absolute private directory without following any symlink.
    /// Root may manage a target user's store only after upstream authorization.
    /// A non-root process may open only its own UID namespace.
    pub fn open(path: &Path, owner_uid: u32, key: SecretKey) -> Result<Self, StorageError> {
        // SAFETY: geteuid takes no pointers and has no preconditions.
        let effective_uid = unsafe { libc::geteuid() };
        if effective_uid != 0 && owner_uid != effective_uid {
            return Err(StorageError::OwnerMismatch);
        }
        let directory = open_directory(path, false, effective_uid)?;
        Ok(Self {
            directory,
            owner_uid,
            effective_uid,
            key,
        })
    }

    /// Explicit user-local passphrase storage. The salt is public, never a key.
    /// Holds an exclusive advisory lock across read/verify/save/delete operations.
    pub fn open_passphrase(
        path: &Path,
        password: &[u8],
        create: bool,
    ) -> Result<Self, StorageError> {
        if !(12..=1024).contains(&password.len()) {
            return Err(StorageError::InvalidPassphrase);
        }
        // SAFETY: geteuid takes no pointers and has no preconditions.
        let uid = unsafe { libc::geteuid() };
        let directory = open_directory(path, create, uid)?;
        lock_directory(&directory)?;
        let salt = read_or_create_salt(&directory, uid, create)?;
        let key = super::passphrase_key(password, &salt)?;
        Ok(Self {
            directory,
            owner_uid: uid,
            effective_uid: uid,
            key,
        })
    }

    /// Deletes the current user's diagnostic reference without needing its key.
    pub fn remove_local_reference(path: &Path) -> Result<bool, StorageError> {
        // SAFETY: geteuid takes no pointers and has no preconditions.
        let uid = unsafe { libc::geteuid() };
        let store = match Self::open(path, uid, SecretKey::from_bytes([0; 32])) {
            Ok(store) => store,
            Err(StorageError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        lock_directory(&store.directory)?;
        store.remove()
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

fn lock_directory(directory: &OwnedFd) -> Result<(), StorageError> {
    // SAFETY: flock operates on a live owned descriptor, without pointers.
    if unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn open_directory(path: &Path, create: bool, uid: u32) -> Result<OwnedFd, StorageError> {
    if !path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::CurDir | Component::Prefix(_)
            )
        })
    {
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
                let name = CString::new(name.as_bytes()).map_err(|_| StorageError::InsecurePath)?;
                if create {
                    // SAFETY: live directory descriptor and NUL-terminated basename.
                    let result =
                        unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
                    if result != 0 {
                        let error = std::io::Error::last_os_error();
                        if error.kind() != std::io::ErrorKind::AlreadyExists {
                            return Err(error.into());
                        }
                    }
                }
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
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(StorageError::InsecurePath);
    }
    Ok(directory)
}

fn read_or_create_salt(
    directory: &OwnedFd,
    uid: u32,
    create: bool,
) -> Result<[u8; 16], StorageError> {
    const SALT: &CStr = c"passphrase.salt";
    let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;
    let descriptor = match open_at(directory.as_raw_fd(), SALT, flags, 0) {
        Ok(fd) => fd,
        Err(StorageError::Io(error)) if create && error.kind() == std::io::ErrorKind::NotFound => {
            // Never replace or invent salt for a pre-existing encrypted reference.
            match open_at(directory.as_raw_fd(), TEMPLATE, flags, 0) {
                Err(StorageError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(StorageError::InvalidEnvelope),
            }
            let mut salt = [0u8; 16];
            getrandom::fill(&mut salt).map_err(|_| StorageError::RandomUnavailable)?;
            let fd = open_at(
                directory.as_raw_fd(),
                SALT,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )?;
            let mut pending = Pending {
                directory: directory.as_raw_fd(),
                name: SALT,
                remove: true,
            };
            let mut file = File::from(fd);
            file.write_all(b"LHSALT01")?;
            file.write_all(&salt)?;
            file.sync_all()?;
            sync_directory(directory)?;
            pending.remove = false;
            return Ok(salt);
        }
        Err(error) => return Err(error),
    };
    let file = File::from(descriptor);
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
        || metadata.len() != 24
    {
        return Err(StorageError::InsecurePath);
    }
    let mut bytes = Vec::with_capacity(25);
    file.take(25).read_to_end(&mut bytes)?;
    if bytes.len() != 24 || &bytes[..8] != b"LHSALT01" {
        return Err(StorageError::InvalidEnvelope);
    }
    let mut salt = [0; 16];
    salt.copy_from_slice(&bytes[8..24]);
    Ok(salt)
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

    #[test]
    fn administrator_import_preserves_user_encryption_and_rejects_wrong_owner_or_key() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } != 0 {
            assert!(matches!(
                SecureStore::import_passphrase(path, 1000, b"test reference phrase"),
                Err(StorageError::OwnerMismatch)
            ));
            return;
        }
        let salt = [6_u8; 16];
        let password = b"test reference phrase";
        let key = crate::passphrase_key(password, &salt).unwrap();
        let template = crate::tests::template().bind_capture([8; 32]).unwrap();
        let original = encrypt_template(&key, 1000, &template).unwrap();
        std::fs::write(path.join("template.lht"), &original).unwrap();
        std::fs::write(
            path.join("passphrase.salt"),
            [b"LHSALT01".as_slice(), &salt].concat(),
        )
        .unwrap();
        for entry in [
            path.to_path_buf(),
            path.join("template.lht"),
            path.join("passphrase.salt"),
        ] {
            let name = CString::new(entry.as_os_str().as_bytes()).unwrap();
            // SAFETY: terminated absolute path inside our owned tempdir; test runs as root.
            assert_eq!(unsafe { libc::chown(name.as_ptr(), 1000, 1000) }, 0);
            let mode = if entry == path { 0o700 } else { 0o600 };
            std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let imported = SecureStore::import_passphrase(path, 1000, password).unwrap();
        assert_eq!(imported.capture_binding(), Some([8; 32]));
        assert!(SecureStore::import_passphrase(path, 1001, password).is_err());
        assert!(SecureStore::import_passphrase(path, 1000, b"wrong reference phrase").is_err());
        std::fs::set_permissions(
            path.join("passphrase.salt"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(SecureStore::import_passphrase(path, 1000, password).is_err());
        assert_eq!(std::fs::read(path.join("template.lht")).unwrap(), original);
    }

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
    fn passphrase_reference_survives_reopening_and_remains_encrypted() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("private/nested");
        let password = b"test reference phrase";
        let store = SecureStore::open_passphrase(&path, password, true).unwrap();
        assert!(store.load().unwrap().is_none());
        let template = crate::tests::template().bind_capture([8; 32]).unwrap();
        store.save(&template).unwrap();
        let plaintext = template.to_bytes().unwrap();
        let encrypted = std::fs::read(path.join("template.lht")).unwrap();
        assert!(
            !encrypted
                .windows(plaintext.len())
                .any(|w| w == plaintext.as_slice())
        );
        for name in ["template.lht", "passphrase.salt"] {
            assert_eq!(
                std::fs::metadata(path.join(name)).unwrap().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
        assert!(
            SecureStore::open_passphrase(&path, password, false).is_err(),
            "exclusive operation lock"
        );
        drop(store);
        let wrong =
            SecureStore::open_passphrase(&path, b"different reference phrase", false).unwrap();
        assert!(matches!(wrong.load(), Err(StorageError::InvalidEnvelope)));
        drop(wrong);
        let reopened = SecureStore::open_passphrase(&path, password, false).unwrap();
        let loaded = reopened.load().unwrap().unwrap();
        assert_eq!(loaded.sample_count(), 3);
        assert_eq!(loaded.capture_binding(), Some([8; 32]));
        drop(reopened);
        assert!(SecureStore::remove_local_reference(&path).unwrap());
        assert!(!SecureStore::remove_local_reference(&path).unwrap());
    }

    #[test]
    fn missing_malformed_or_linked_salt_never_rekeys_existing_data() {
        let (directory, store) = setup();
        store.save(&crate::tests::template()).unwrap();
        let path = directory.path();
        let original = std::fs::read(path.join("template.lht")).unwrap();
        assert!(matches!(
            SecureStore::open_passphrase(path, b"test reference phrase", true),
            Err(StorageError::InvalidEnvelope)
        ));
        let salt = path.join("passphrase.salt");
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), &salt).unwrap();
        assert!(SecureStore::open_passphrase(path, b"test reference phrase", true).is_err());
        std::fs::remove_file(&salt).unwrap();
        std::fs::write(&salt, b"malformed").unwrap();
        assert!(SecureStore::open_passphrase(path, b"test reference phrase", true).is_err());
        assert_eq!(std::fs::read(path.join("template.lht")).unwrap(), original);
        assert_eq!(std::fs::metadata(outside.path()).unwrap().len(), 0);
    }

    #[test]
    fn short_passphrase_and_alias_creation_fail_before_writes() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("new");
        assert!(matches!(
            SecureStore::open_passphrase(&path, b"short", true),
            Err(StorageError::InvalidPassphrase)
        ));
        assert!(!path.exists());
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), &path).unwrap();
        assert!(
            SecureStore::open_passphrase(&path.join("nested"), b"test reference phrase", true)
                .is_err()
        );
        assert!(!outside.path().join("nested").exists());
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
