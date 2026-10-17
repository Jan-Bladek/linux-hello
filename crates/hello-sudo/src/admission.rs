//! Persistent per-account cooldown in addition to the global nonblocking lease.
//! Busy/rate-limited requests immediately retain password fallback; no queue.
use crate::{Error, directory, protected_file, protected_path};
use std::{fs, io::Read};

const RESERVATION_NS: u64 = 40_000_000_000;
const COOLDOWN_NS: u64 = 3_000_000_000;
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    uid: u32,
    boot: String,
    next_ns: u64,
}
impl State {
    fn allows(&self, uid: u32, boot: &str, now: u64) -> Result<bool, Error> {
        if self.version != 1 || self.uid != uid {
            return Err("Invalid admission record".into());
        }
        Ok(self.boot != boot || now >= self.next_ns)
    }
}
fn clock() -> Result<(String, u64), Error> {
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned();
    if boot.len() != 36 || !boot.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("Invalid kernel boot identity".into());
    }
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: initialized writable timespec and a constant monotonic boot clock.
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut value) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let now = u64::try_from(value.tv_sec)?
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(u64::try_from(value.tv_nsec).ok()?))
        .ok_or("Boot clock overflow")?;
    Ok((boot, now))
}
fn read(uid: u32) -> Result<Option<Vec<u8>>, Error> {
    match protected_file(&directory(uid).join(".admission.json"), true, 256) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(257).read_to_end(&mut bytes)?;
            Ok(Some(bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn write(uid: u32, bytes: &[u8]) -> Result<(), Error> {
    let dir = directory(uid);
    let path = dir.join(".admission.json");
    protected_path(&path)?;
    if fs::symlink_metadata(&path).is_ok() {
        protected_file(&path, true, 256)?;
    }
    let mut nonce = [0; 8];
    getrandom::fill(&mut nonce)?;
    let pending = dir.join(format!(".admission-{:016x}", u64::from_le_bytes(nonce)));
    let result = (|| {
        super::profile::write_private(&pending, bytes)?;
        fs::rename(&pending, path)?;
        fs::File::open(dir)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(pending);
    result
}
pub(super) struct Admission {
    uid: u32,
    reservation: Vec<u8>,
}
impl Admission {
    /// Caller holds AttemptLease for the entire lifetime of this reservation.
    pub(super) fn reserve(uid: u32) -> Result<Self, Error> {
        if uid < 1000 {
            return Err("Normal account required".into());
        }
        let (boot, now) = clock()?;
        if let Some(bytes) = read(uid)? {
            let previous: State = serde_json::from_slice(&bytes)?;
            if !previous.allows(uid, &boot, now)? {
                return Err("Face verification cooldown; use your password".into());
            }
        }
        let reservation = serde_json::to_vec(&State {
            version: 1,
            uid,
            boot,
            next_ns: now.saturating_add(RESERVATION_NS),
        })?;
        write(uid, &reservation)?;
        Ok(Self { uid, reservation })
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        let result = (|| -> Result<(), Error> {
            // Do not overwrite an administrator's edit made during capture.
            if read(self.uid)?.as_deref() != Some(self.reservation.as_slice()) {
                return Ok(());
            }
            let (boot, now) = clock()?;
            write(
                self.uid,
                &serde_json::to_vec(&State {
                    version: 1,
                    uid: self.uid,
                    boot,
                    next_ns: now.saturating_add(COOLDOWN_NS),
                })?,
            )
        })();
        if result.is_err() {
            eprintln!("Linux Hello: admission cleanup failed; reserved cooldown retained.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooldown_is_uid_bound_monotonic_and_resets_on_new_boot() {
        let state = State {
            version: 1,
            uid: 1000,
            boot: "boot-a".into(),
            next_ns: 5000,
        };
        assert!(!state.allows(1000, "boot-a", 4999).unwrap());
        assert!(state.allows(1000, "boot-a", 5000).unwrap());
        assert!(state.allows(1000, "boot-b", 1).unwrap());
        assert!(state.allows(1001, "boot-a", 6000).is_err());
        assert!(Admission::reserve(0).is_err());
    }
}
