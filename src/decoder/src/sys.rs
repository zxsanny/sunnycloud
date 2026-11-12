//! The runner's only unsafe code: two libc calls the standard library does not wrap.

use std::io;

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
