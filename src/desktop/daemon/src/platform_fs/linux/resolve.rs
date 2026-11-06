use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use rustix::fs::{CWD, FileType, Mode, OFlags, ResolveFlags, fstat, openat, openat2};
use rustix::io::Errno;

use super::super::{FsError, RelPath};

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// Opens names beneath a directory handle without ever following a symlink or leaving the handle:
/// one `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` when the kernel has it, otherwise one
/// `openat(O_NOFOLLOW)` per component.
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
            Err(e) => {
                tracing::warn!(error = %e, "openat2 unavailable, resolving names component by component");
                Self { openat2: false }
            }
        }
    }

    pub(super) fn component_walk() -> Self {
        Self { openat2: false }
    }

    pub(super) fn uses_openat2(&self) -> bool {
        self.openat2
    }

    /// `flags` apply to the last component. `CLOEXEC` and `NOFOLLOW` are always added.
    ///
    /// `O_DIRECTORY | O_NOFOLLOW` reports a symlink as `ENOTDIR`, indistinguishable from a regular file
    /// without a racy second look. So `O_DIRECTORY` is never passed: a symlink is then `ELOOP`, and the
    /// type of the descriptor actually obtained is checked.
    pub(super) fn open(
        &self,
        dir: BorrowedFd<'_>,
        path: &RelPath,
        flags: OFlags,
        mode: Mode,
    ) -> Result<OwnedFd, FsError> {
        let want_dir = flags.contains(OFlags::DIRECTORY);
        let flags = no_follow(flags.difference(OFlags::DIRECTORY), want_dir);
        let fd = if self.openat2 {
            let resolve = ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS;
            openat2(dir, path.as_string(), flags, mode, resolve).map_err(map_errno)?
        } else {
            open_by_components(dir, path, flags, mode)?
        };
        if want_dir {
            require_dir(&fd)?;
        }
        Ok(fd)
    }

    /// A handle for the directory holding `path`'s leaf (a duplicate of `dir` when the leaf is directly in it).
    pub(super) fn open_parent(
        &self,
        dir: BorrowedFd<'_>,
        path: &RelPath,
    ) -> Result<OwnedFd, FsError> {
        let parent = path.parent();
        if parent.is_empty() {
            return dir.try_clone_to_owned().map_err(FsError::from);
        }
        self.open(
            dir,
            &parent,
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )
    }
}

fn open_by_components(
    dir: BorrowedFd<'_>,
    path: &RelPath,
    flags: OFlags,
    mode: Mode,
) -> Result<OwnedFd, FsError> {
    let (leaf, parents) = path
        .components()
        .split_last()
        .ok_or(FsError::NameNotRepresentable)?;
    let mut held: Option<OwnedFd> = None;
    for part in parents {
        let cur = held.as_ref().map_or(dir, |fd| fd.as_fd());
        let next = openat(
            cur,
            part.as_str(),
            no_follow(OFlags::RDONLY, true),
            Mode::empty(),
        )
        .map_err(map_errno)?;
        require_dir(&next)?;
        held = Some(next);
    }
    let cur = held.as_ref().map_or(dir, |fd| fd.as_fd());
    openat(cur, leaf.as_str(), flags, mode).map_err(map_errno)
}

/// `NONBLOCK` keeps a wrongly typed name (a FIFO where a directory is expected) from hanging the open.
fn no_follow(flags: OFlags, nonblock: bool) -> OFlags {
    let flags = flags | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    if nonblock {
        flags | OFlags::NONBLOCK
    } else {
        flags
    }
}

fn require_dir(fd: &OwnedFd) -> Result<(), FsError> {
    let mode = fstat(fd).map_err(map_errno)?.st_mode;
    if FileType::from_raw_mode(mode) == FileType::Directory {
        Ok(())
    } else {
        Err(map_errno(Errno::NOTDIR))
    }
}

pub(super) fn map_errno(e: Errno) -> FsError {
    match e {
        Errno::XDEV | Errno::LOOP => FsError::EscapesRoot,
        Errno::ACCESS | Errno::PERM => FsError::PermissionDenied,
        Errno::NAMETOOLONG | Errno::ILSEQ => FsError::NameNotRepresentable,
        other => FsError::Io(other.into()),
    }
}
