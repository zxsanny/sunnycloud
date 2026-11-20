//! The runner's only unsafe code: the libc calls the standard library does not wrap.

use std::io;
use std::os::fd::RawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// `open(2)` flags for `OpenOptionsExt::custom_flags`.
pub use libc::{O_NOFOLLOW, O_NONBLOCK, O_PATH};

/// Marks the process non-dumpable: no core dump is made on `abort()` and no other process can
/// `ptrace` it or read its memory. A core dump is not just slow, it is delayed by the host's
/// core handler (a `core_pattern` pipe such as apport runs even with `RLIMIT_CORE=0`), and the
/// watchdog's abort must end the process within a fraction of a second (10 section 13.6).
pub fn disable_dumps() -> io::Result<()> {
    // SAFETY: PR_SET_DUMPABLE takes one integer argument (0 or 1) and the call has no pointer
    // arguments; the unused arguments are not read by the kernel for this option.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Makes the runner the parent of every orphaned descendant (`PR_SET_CHILD_SUBREAPER`), so a
/// double-forked process cannot reparent to the container's init out of the runner's reach.
pub fn set_child_subreaper() -> io::Result<()> {
    // SAFETY: PR_SET_CHILD_SUBREAPER takes one integer argument (1 enables); no pointers.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// SIGKILLs one process. One that is already gone is not an error.
pub fn kill_process(pid: u32) -> io::Result<()> {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    // SAFETY: `kill` takes two integers; no pointers are involved.
    if unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Reaps every child that has already ended (zombies, including the reparented descendants of
/// a subreaper) and returns how many it reaped.
pub fn reap_ended_children() -> io::Result<usize> {
    let mut reaped = 0;
    loop {
        let mut status = 0;
        // SAFETY: `status` is a valid `c_int` that outlives the call.
        let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        match pid {
            0 => return Ok(reaped),
            1.. => reaped += 1,
            _ => {
                let error = io::Error::last_os_error();
                return match error.raw_os_error() {
                    Some(libc::ECHILD) => Ok(reaped),
                    Some(libc::EINTR) => continue,
                    _ => Err(error),
                };
            }
        }
    }
}

/// Installs `handler` for `SIGUSR1`. The handler must be async-signal-safe: the caller passes
/// one that only stores to an atomic.
#[cfg(feature = "test-freeze")]
pub fn on_sigusr1(handler: extern "C" fn(libc::c_int)) -> io::Result<()> {
    // SAFETY: `handler` is a plain `extern "C"` function that lives for the whole process and
    // only stores to an atomic, so it is async-signal-safe.
    if unsafe { libc::signal(libc::SIGUSR1, handler as libc::sighandler_t) } == libc::SIG_ERR {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// The descriptor number the job input has inside a child. A child opens it as
/// `/proc/self/fd/<INPUT_FD>`: a path made of fixed text, with no job path in it.
pub const INPUT_FD: i32 = 9;

/// What a child is confined to before `exec`.
#[derive(Debug, Clone, Copy)]
pub struct ChildLimits {
    pub max_file_bytes: u64,
    pub cpu_seconds: u64,
    pub open_files: u64,
}

/// Installs, between `fork` and `exec` of `command`, the resource limits (no core dump, file
/// size, CPU backstop, open files; never `RLIMIT_AS`) and maps `input_fd` to [`INPUT_FD`]
/// without close-on-exec.
pub fn confine(command: &mut Command, limits: ChildLimits, input_fd: RawFd) {
    // SAFETY: the closure runs in the forked child before `exec` and calls only `setrlimit`,
    // `dup2` and `fcntl`, which are async-signal-safe; it allocates nothing and takes no lock.
    unsafe {
        command.pre_exec(move || {
            set_limit(libc::RLIMIT_CORE, 0)?;
            set_limit(libc::RLIMIT_FSIZE, limits.max_file_bytes)?;
            set_limit(libc::RLIMIT_CPU, limits.cpu_seconds)?;
            set_limit(libc::RLIMIT_NOFILE, limits.open_files)?;
            if libc::dup2(input_fd, INPUT_FD) < 0 || libc::fcntl(INPUT_FD, libc::F_SETFD, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn set_limit(resource: libc::__rlimit_resource_t, value: u64) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: `limit` is a valid `rlimit` that outlives the call.
    if unsafe { libc::setrlimit(resource, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// SIGKILLs every process of group `pgid`. A group that is already empty is not an error.
pub fn kill_group(pgid: u32) -> io::Result<()> {
    let Ok(pgid) = libc::pid_t::try_from(pgid) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    // SAFETY: `kill` with a negative pid addresses a process group; no pointers are involved.
    if unsafe { libc::kill(-pgid, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Bytes per memory page, for the `rss` field of `/proc/<pid>/stat`.
pub fn page_size() -> u64 {
    // SAFETY: `sysconf` takes one integer and returns a value; no pointers are involved.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    u64::try_from(size).unwrap_or(4096)
}
