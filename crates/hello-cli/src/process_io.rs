//! Deadline covers every pipe operation, independently of child/descendant EOF.
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    time::{Duration, Instant},
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
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
