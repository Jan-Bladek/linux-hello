//! Deadline covers every pipe operation, independently of child/descendant EOF.
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    time::{Duration, Instant},
};

/// Reduce accidental secret persistence in dumps. This cannot wipe copies in
/// the renderer/native allocator or protect against an already privileged reader.
pub fn protect_secret_process() -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: initialized limit and constant resource/prctl arguments.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: constant prctl arguments, no pointers.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub struct DeadlineIo<T> {
    inner: T,
    deadline: Instant,
}
impl<T: AsRawFd> DeadlineIo<T> {
    pub fn new(inner: T, deadline: Instant) -> io::Result<Self> {
        // SAFETY: fcntl uses a live descriptor borrowed from inner.
        let flags = unsafe { libc::fcntl(inner.as_raw_fd(), libc::F_GETFL) };
        // SAFETY: same live descriptor; only nonblocking mode is added.
        if flags < 0
            // SAFETY: same live descriptor; only nonblocking mode is added.
            || unsafe { libc::fcntl(inner.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { inner, deadline })
    }
    fn wait(&self) -> io::Result<()> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Pipe deadline exceeded",
            ));
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
        Ok(())
    }
}
impl<T: Read + AsRawFd> Read for DeadlineIo<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            if Instant::now() >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Pipe deadline exceeded",
                ));
            }
            match self.inner.read(bytes) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.wait()?
                }
                result => return result,
            }
        }
    }
}
impl<T: Write + AsRawFd> Write for DeadlineIo<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            if Instant::now() >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Pipe deadline exceeded",
                ));
            }
            match self.inner.write(bytes) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.wait()?
                }
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Bounded ordinary-user helper supervision. Root provisioning has its own
/// recovery protocol and must not be run through this process-group killer.
pub fn bounded_output(
    command: &mut std::process::Command,
    deadline: Instant,
    maximum: usize,
) -> io::Result<std::process::Output> {
    use std::{os::unix::process::CommandExt, process::Stdio, sync::mpsc};
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Operation deadline exceeded",
        ));
    }
    let mut child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let pid = child.id();
    let output = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let result = (|| {
            let mut bytes = Vec::new();
            DeadlineIo::new(output, deadline)?
                .take(maximum as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > maximum {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Helper output exceeds limit",
                ));
            }
            Ok(bytes)
        })();
        let _ = sender.send(result);
    });
    let result = (|| {
        let mut status = None;
        let mut bytes = None;
        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Helper deadline exceeded",
                ));
            }
            if bytes.is_none() {
                match receiver.try_recv() {
                    Ok(result) => bytes = Some(result?),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err(io::Error::other("Helper reader failed"));
                    }
                    Err(mpsc::TryRecvError::Empty) => (),
                }
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if let (Some(status), Some(stdout)) = (status, bytes.as_mut()) {
                return Ok(std::process::Output {
                    status,
                    stdout: std::mem::take(stdout),
                    stderr: Vec::new(),
                });
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    // This dedicated group contains only the ordinary helper and descendants;
    // close descendants that retained stdout even after their parent exited.
    // SAFETY: positive spawned PID is used as the dedicated process-group ID.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    let _ = child.wait();
    let _ = reader.join(); // DeadlineIo bounds EOF, including descendants.
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    #[test]
    fn ordinary_helpers_bound_hangs_descendant_eof_output_and_total_deadline() {
        for script in ["sleep 60", "sleep 60 & exit 0", "printf 123456789"] {
            let started = Instant::now();
            let error = bounded_output(
                std::process::Command::new("/bin/sh").args(["-c", script]),
                started + Duration::from_millis(80),
                4,
            )
            .unwrap_err();
            assert!(matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::InvalidData
            ));
            assert!(started.elapsed() < Duration::from_secs(2));
        }
        let output = bounded_output(
            std::process::Command::new("/bin/sh").args(["-c", "printf login"]),
            Instant::now() + Duration::from_secs(2),
            16,
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"login");
        assert!(
            bounded_output(
                &mut std::process::Command::new("/bin/true"),
                Instant::now(),
                16
            )
            .is_err()
        );
    }
    #[test]
    fn partial_input_and_descendant_held_eof_are_bounded() {
        let (read, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"x").unwrap();
        let mut read = DeadlineIo::new(read, Instant::now() + Duration::from_millis(40)).unwrap();
        let started = Instant::now();
        let mut bytes = Vec::new();
        assert_eq!(
            read.read_to_end(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(bytes, b"x");
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn nonreading_peer_write_is_bounded() {
        let (write, _peer) = UnixStream::pair().unwrap();
        let mut write = DeadlineIo::new(write, Instant::now() + Duration::from_millis(40)).unwrap();
        assert_eq!(
            write.write_all(&vec![0; 1024 * 1024]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }
}
