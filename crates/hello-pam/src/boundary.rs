//! Small Linux filesystem and account boundary shared by the PAM adapter/helper.
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path},
};

pub const HELPER: &str = "/usr/local/libexec/linux-hello-sudo/hello-sudo";
pub const ROOT: &str = "/var/lib/linux-hello/sudo";
pub const MODELS: &str = "/usr/local/libexec/linux-hello-sudo/models";

/// Every ancestor must be a real root-owned directory, never writable by others.
pub fn protected_path(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let mut current = std::path::PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                let meta = std::fs::symlink_metadata(&current)?;
                if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                current.push(name);
            }
            _ => return Err(io::ErrorKind::PermissionDenied.into()),
        }
    }
    Ok(())
}
pub fn protected_file(path: &Path, private: bool, max: u64) -> io::Result<File> {
    protected_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let meta = file.metadata()?;
    let forbidden = if private { 0o077 } else { 0o022 };
    if !meta.is_file()
        || meta.uid() != 0
        || meta.nlink() != 1
        || meta.mode() & forbidden != 0
        || meta.len() > max
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(file)
}
pub fn account(name: &str) -> io::Result<(u32, u32, std::path::PathBuf)> {
    let name = CString::new(name).map_err(|_| io::ErrorKind::InvalidInput)?;
    let mut buffer = vec![0_u8; 65536];
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    // SAFETY: all pointers refer to writable storage; name is terminated. NSS
    // keeps string pointers inside the buffer until we copy them below.
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(io::ErrorKind::NotFound.into());
    }
    // SAFETY: successful getpwnam_r initialized record and its string pointers.
    let record = unsafe { record.assume_init() };
    if record.pw_dir.is_null() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // SAFETY: NSS returned a terminated directory string inside the live buffer.
    let home = unsafe { std::ffi::CStr::from_ptr(record.pw_dir) }
        .to_str()
        .map_err(|_| io::ErrorKind::InvalidData)?;
    Ok((record.pw_uid, record.pw_gid, home.into()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_relative_parent_and_user_controlled_paths() {
        assert!(protected_path(Path::new("relative/file")).is_err());
        assert!(protected_path(Path::new("/usr/../etc/passwd")).is_err());
        let temp = tempfile::tempdir().unwrap();
        assert!(protected_file(&temp.path().join("test"), false, 100).is_err());
        assert!(account("missing-linux-hello-test-account-938472").is_err());
    }
}
