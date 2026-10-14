//! Linux-only resource and syscall boundary. No unsafe code outside this module.
use std::{io, os::unix::process::CommandExt, process::Command};

pub const ADDRESS_SPACE_BYTES: u64 = 1024 * 1024 * 1024;
pub const CPU_SECONDS: u64 = 5;
pub const STREAM_CPU_SECONDS: u64 = 20;

pub fn apply_limits() -> io::Result<()> {
    apply_limits_with_cpu(CPU_SECONDS)
}
pub fn apply_stream_limits() -> io::Result<()> {
    apply_limits_with_cpu(STREAM_CPU_SECONDS)
}
fn apply_limits_with_cpu(cpu: u64) -> io::Result<()> {
    for (resource, value) in [
        (libc::RLIMIT_AS, ADDRESS_SPACE_BYTES),
        (libc::RLIMIT_CPU, cpu),
        (libc::RLIMIT_CORE, 0),
        (libc::RLIMIT_FSIZE, 0),
        (libc::RLIMIT_NOFILE, 32),
    ] {
        let limit = libc::rlimit {
            rlim_cur: value,
            rlim_max: value,
        };
        // SAFETY: limit is a live Linux rlimit; all resources and values are constants.
        if unsafe { libc::setrlimit(resource, &limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: integer-only prctl commands; no pointers and no additional arguments used.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0
        // SAFETY: PR_SET_DUMPABLE takes an integer mode; no pointer arguments.
        || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn configure(command: &mut Command) {
    configure_with_cpu(command, CPU_SECONDS);
}
pub fn configure_stream(command: &mut Command) {
    configure_with_cpu(command, STREAM_CPU_SECONDS);
}
fn configure_with_cpu(command: &mut Command, cpu: u64) {
    // Parent thread remains alive for the lifetime of the CLI. The closure uses
    // only async-signal-safe syscalls, constants and stack values after fork.
    // SAFETY: no allocation, locks or shared mutable Rust state are touched in pre_exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            let parent = libc::getppid();
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if parent == 1 || libc::getppid() != parent {
                return Err(io::Error::from_raw_os_error(libc::ECHILD));
            }
            // Mark unexpected inherited descriptors close-on-exec; the standard
            // process spawn implementation retains its own exec-error pipe correctly.
            if libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 4_u32) != 0 {
                return Err(io::Error::last_os_error());
            }
            apply_limits_with_cpu(cpu)
        });
    }
}

pub fn nonblocking(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fd is an owned live pipe descriptor; F_GETFL/F_SETFL take integer arguments.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: same live descriptor, adding only O_NONBLOCK to the original flags.
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn kill_group(pid: u32) {
    // SAFETY: the child created a new session with PGID=PID; its group belongs
    // only to this supervised worker, not the CLI or the user's shell.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

pub fn peak_rss_kib() -> u64 {
    // SAFETY: rusage is a C struct for which zero initialization is valid.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: exclusive pointer to a live rusage, with the valid RUSAGE_SELF selector.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0 {
        usage.ru_maxrss.max(0) as u64
    } else {
        0
    }
}

pub fn cpu_time_us() -> u64 {
    // SAFETY: zero is valid for the C rusage structure.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: exclusive pointer to live rusage with a valid selector.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return 0;
    }
    let micros = |value: libc::timeval| {
        (value.tv_sec.max(0) as u64)
            .saturating_mul(1_000_000)
            .saturating_add(value.tv_usec.max(0) as u64)
    };
    micros(usage.ru_utime).saturating_add(micros(usage.ru_stime))
}

pub fn restrict_syscalls() -> io::Result<()> {
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return Err(io::Error::other("unsupported worker syscall architecture"));
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        fn instruction(code: u16, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
            libc::sock_filter { code, jt, jf, k }
        }
        const LOAD: u16 = 0x20;
        const EQUAL: u16 = 0x15;
        const RETURN: u16 = 0x06;
        let mut program = vec![
            instruction(LOAD, 0, 0, 4),
            instruction(EQUAL, 1, 0, ARCH),
            instruction(RETURN, 0, 0, libc::SECCOMP_RET_KILL_PROCESS),
            instruction(LOAD, 0, 0, 0),
        ];
        // Model files are already verified and closed. CPU inference and private
        // pipes need no open/socket/exec/clone/ioctl/path mutation syscalls.
        let allowed = [
            libc::SYS_read,
            libc::SYS_write,
            libc::SYS_readv,
            libc::SYS_writev,
            libc::SYS_close,
            libc::SYS_fstat,
            libc::SYS_lseek,
            libc::SYS_pread64,
            libc::SYS_brk,
            libc::SYS_mmap,
            libc::SYS_munmap,
            libc::SYS_mprotect,
            libc::SYS_mremap,
            libc::SYS_madvise,
            libc::SYS_futex,
            libc::SYS_rt_sigaction,
            libc::SYS_rt_sigprocmask,
            libc::SYS_rt_sigreturn,
            libc::SYS_sigaltstack,
            libc::SYS_clock_gettime,
            libc::SYS_gettimeofday,
            libc::SYS_nanosleep,
            libc::SYS_clock_nanosleep,
            libc::SYS_sched_yield,
            libc::SYS_sched_getaffinity,
            libc::SYS_getpid,
            libc::SYS_gettid,
            libc::SYS_getuid,
            libc::SYS_geteuid,
            libc::SYS_getgid,
            libc::SYS_getegid,
            libc::SYS_getrandom,
            libc::SYS_getrusage,
            libc::SYS_uname,
            libc::SYS_fcntl,
            libc::SYS_exit,
            libc::SYS_exit_group,
        ];
        for number in allowed {
            program.push(instruction(EQUAL, 0, 1, number as u32));
            program.push(instruction(RETURN, 0, 0, libc::SECCOMP_RET_ALLOW));
        }
        program.push(instruction(
            RETURN,
            0,
            0,
            libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
        ));
        let descriptor = libc::sock_fprog {
            len: program.len() as u16,
            filter: program.as_mut_ptr(),
        };
        // SAFETY: descriptor points to the live, bounded BPF instruction array.
        // The kernel copies it synchronously; no_new_privs was set before this call.
        if unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &descriptor) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Verified model bytes are already in memory. Native parsing must not run as root.
pub fn drop_privileges() -> io::Result<()> {
    // SAFETY: getppid is an integer-only process query.
    let parent = unsafe { libc::getppid() };
    // SAFETY: credential syscalls take only constants; no shared Rust state is touched.
    if unsafe { libc::geteuid() } == 0 {
        // Use the reserved overflow/nobody identity, without NSS or inherited groups.
        // SAFETY: zero groups allows a null pointer; constant UID/GID are non-root.
        if unsafe { libc::prctl(libc::PR_SET_KEEPCAPS, 0, 0, 0, 0) } != 0
            // SAFETY: zero groups permits a null array pointer.
            || unsafe { libc::setgroups(0, std::ptr::null()) } != 0
            // SAFETY: all three group IDs are set to the unprivileged reserved identity.
            || unsafe { libc::setresgid(65534, 65534, 65534) } != 0
            // SAFETY: all three user IDs are dropped permanently, before native parsing.
            || unsafe { libc::setresuid(65534, 65534, 65534) } != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    // Changing credentials can reset dumpability; protect the process again.
    // SAFETY: PR_SET_DUMPABLE takes an integer and no pointers.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // Credential changes clear PDEATHSIG. Rearm it after the permanent drop.
    // SAFETY: integer-only prctl/getppid commands; no pointers are involved.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: getppid takes no pointers; check the registration race.
    if parent == 1 || unsafe { libc::getppid() } != parent {
        return Err(io::Error::from_raw_os_error(libc::ECHILD));
    }
    Ok(())
}

#[cfg(test)]
mod audit_tests {
    #[test]
    #[ignore = "root, current release worker and installed pinned models required"]
    fn root_worker_drops_all_credentials_before_real_model_parsing() {
        // SAFETY: geteuid is an integer-only credential query.
        assert_eq!(unsafe { libc::geteuid() }, 0);
        let executable = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("hello-face-worker");
        let worker = crate::face_worker::Worker::spawn_stream(
            &executable,
            std::path::Path::new("/usr/local/libexec/linux-hello-sudo/models"),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .unwrap()
        .initialize()
        .unwrap();
        // READY is emitted only after verified bytes, privilege drop, seccomp and
        // native model construction in the real single-threaded worker executable.
        let status =
            std::fs::read_to_string(format!("/proc/{}/status", worker.child.id())).unwrap();
        for field in ["Uid:", "Gid:"] {
            let ids = status
                .lines()
                .find(|line| line.starts_with(field))
                .unwrap()
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>();
            assert_eq!(ids, vec!["65534"; 4]);
        }
        assert_eq!(
            status
                .lines()
                .find(|line| line.starts_with("Groups:"))
                .unwrap()
                .split_whitespace()
                .count(),
            1
        );
        for field in ["CapPrm:", "CapEff:", "CapAmb:"] {
            assert_eq!(
                status
                    .lines()
                    .find(|line| line.starts_with(field))
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap(),
                "0000000000000000"
            );
        }
        assert_eq!(
            status
                .lines()
                .find(|line| line.starts_with("NoNewPrivs:"))
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap(),
            "1"
        );
        worker.finish().unwrap();
    }
}
