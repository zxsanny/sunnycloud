use std::fs::File;
use std::io::{self, Read, Take};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};

use rustix::fs::{CWD, FileType, Mode, OFlags, ResolveFlags, fstat, open, openat, openat2};
use rustix::io::Errno;

use super::{RejectReason, ScratchError};

const PATH_FLAGS: OFlags = OFlags::PATH.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
const DIR_FLAGS: OFlags = OFlags::PATH
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// Opens a name beneath a directory handle without following a symlink or leaving the handle: one
/// `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` when the kernel (and the container's seccomp profile)
/// allows it, otherwise one `openat(O_NOFOLLOW)` per component.
#[derive(Clone, Copy, Debug)]
pub(super) struct Resolver {
    openat2: bool,
}

impl Resolver {
    pub(super) fn detect() -> Self {
        let probe = openat2(
            CWD,
            ".",
            DIR_FLAGS,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
        );
        match probe {
            Ok(_) => Self { openat2: true },
            Err(error) => {
                sc_observability::log_warn!(
                    "openat2 unavailable, resolving output names component by component",
                    err = &io::Error::from(error)
                );
                Self { openat2: false }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn component_walk() -> Self {
        Self { openat2: false }
    }

    /// An `O_PATH` handle on `rel` beneath `dir`. A symlink leaf is refused by `openat2` (`ELOOP`) and, in the
    /// component walk, handed back as the link itself for the caller's type check.
    fn open_path(&self, dir: BorrowedFd<'_>, rel: &str) -> Result<OwnedFd, Errno> {
        if self.openat2 {
            return openat2(
                dir,
                rel,
                PATH_FLAGS,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
            );
        }
        let mut parts: Vec<&str> = rel.split('/').collect();
        let leaf = parts.pop().unwrap_or_default();
        // `..` and empty or `.` parts would leave the handle, which RESOLVE_BENEATH forbids on the other path.
        if parts
            .iter()
            .chain(std::iter::once(&leaf))
            .any(|part| matches!(*part, "" | "." | ".."))
        {
            return Err(Errno::XDEV);
        }
        let mut held: Option<OwnedFd> = None;
        for part in parts {
            let cur = held.as_ref().map_or(dir, |fd| fd.as_fd());
            held = Some(openat(cur, part, DIR_FLAGS, Mode::empty())?);
        }
        let cur = held.as_ref().map_or(dir, |fd| fd.as_fd());
        openat(cur, leaf, PATH_FLAGS, Mode::empty())
    }
}

/// A decoder output that passed the safe-open checks: a regular, single-link file of at most the cap, opened
/// read-only and non-blocking.
#[derive(Debug)]
pub struct OutputFile {
    pub(super) file: File,
    size: u64,
}

impl OutputFile {
    /// The size `fstat` reported, never the decoder's own figure.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Reads at most `size` bytes, so a file that grows after the check cannot exceed the cap.
    pub fn into_reader(self) -> Take<File> {
        self.file.take(self.size)
    }
}

/// Resolves `rel` beneath `dir`, checks the file behind the handle, then reopens that very file through the
/// handle (so a name swapped in the meantime is never looked at) and checks it again.
pub(super) fn open_output(
    resolver: &Resolver,
    dir: BorrowedFd<'_>,
    rel: &str,
    cap: u64,
) -> Result<OutputFile, ScratchError> {
    let pinned = resolver.open_path(dir, rel).map_err(open_error)?;
    vet(&pinned, cap)?;
    let file = File::from(reopen(&pinned)?);
    let size = vet(&file, cap)?;
    Ok(OutputFile { file, size })
}

fn reopen(pinned: &OwnedFd) -> Result<OwnedFd, ScratchError> {
    let magic_link = format!("/proc/self/fd/{}", pinned.as_raw_fd());
    open(
        magic_link.as_str(),
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(open_error)
}

fn vet(fd: impl AsFd, cap: u64) -> Result<u64, ScratchError> {
    let stat =
        fstat(fd).map_err(|errno| ScratchError::io("cannot inspect the decoder output", errno))?;
    match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => {}
        FileType::Symlink => return Err(RejectReason::Symlink.into()),
        _ => return Err(RejectReason::NotRegular.into()),
    }
    if stat.st_nlink != 1 {
        return Err(RejectReason::Hardlinked.into());
    }
    match u64::try_from(stat.st_size) {
        Ok(size) if size <= cap => Ok(size),
        _ => Err(RejectReason::TooLarge.into()),
    }
}

fn open_error(errno: Errno) -> ScratchError {
    match errno {
        Errno::LOOP => RejectReason::Symlink.into(),
        Errno::NOENT => RejectReason::Missing.into(),
        Errno::NOTDIR
        | Errno::ISDIR
        | Errno::XDEV
        | Errno::ACCESS
        | Errno::PERM
        | Errno::NXIO
        | Errno::NAMETOOLONG => RejectReason::Unusable.into(),
        other => ScratchError::io("cannot open the decoder output", other),
    }
}
