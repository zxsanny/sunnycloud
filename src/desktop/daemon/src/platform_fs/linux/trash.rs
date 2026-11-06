//! Freedesktop trash specification 1.0: the home trash (`$XDG_DATA_HOME/Trash`) for files on its volume,
//! `<mount point>/.Trash-<uid>` for every other volume. A file is renamed into `files/` and described by
//! `info/<name>.trashinfo`; it is never copied or unlinked. Trash locations are reached through
//! directory handles; the data-home path is only ever opened read-only.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rustix::fs::{
    AtFlags, CWD, Mode, OFlags, RenameFlags, fstat, mkdirat, openat, renameat_with, statat,
    unlinkat,
};
use rustix::io::Errno;
use rustix::process::geteuid;

use super::super::{FsError, RelPath};
use super::LinuxDir;
use super::local_time::local_timestamp;
use super::resolve::{Resolver, map_errno};

const DIR_MODE: u32 = 0o700;
const INFO_MODE: u32 = 0o600;
/// Room for `.trashinfo` (10 bytes) and a `.N` collision suffix inside a 255-byte file name.
const MAX_TRASH_NAME_BYTES: usize = 200;
const MAX_NAME_ATTEMPTS: u32 = 1000;
const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

struct TrashDirs {
    files: OwnedFd,
    info: OwnedFd,
    /// The mount point of a volume trash; `Path=` is written relative to it. `None`: absolute.
    topdir: Option<PathBuf>,
}

pub(super) fn trash(
    resolver: &Resolver,
    data_home: Option<&Path>,
    now: SystemTime,
    dir: &LinuxDir,
    rel: &RelPath,
) -> Result<(), FsError> {
    let parent = resolver.open_parent(dir.fd.as_fd(), rel)?;
    let leaf = rel.leaf();
    let stat = statat(&parent, leaf, AtFlags::SYMLINK_NOFOLLOW).map_err(map_errno)?;
    let original = dir.path.join(rel.as_string());
    let dirs = locate(&parent, original.parent(), stat.st_dev, data_home)?;
    let date = local_timestamp(now)?;
    move_into(&parent, leaf, &original, &date, &dirs)
}

fn locate(
    parent: &OwnedFd,
    parent_path: Option<&Path>,
    dev: u64,
    data_home: Option<&Path>,
) -> Result<TrashDirs, FsError> {
    if let Some(home) = data_home
        && let Some(home_fd) = open_if_same_volume(home, dev)
    {
        return unusable_as_unavailable(trash_dirs(&home_fd, "Trash", None), "home trash");
    }
    let (top, topdir) =
        unusable_as_unavailable(find_mount_point(parent, parent_path, dev), "volume")?;
    let name = format!(".Trash-{}", geteuid().as_raw());
    unusable_as_unavailable(trash_dirs(&top, &name, topdir), "volume trash")
}

/// `None` when the data home cannot be opened or sits on another volume than the file.
fn open_if_same_volume(home: &Path, dev: u64) -> Option<OwnedFd> {
    match openat(
        CWD,
        home,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => match fstat(&fd) {
            Ok(st) if st.st_dev == dev => Some(fd),
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(error = %e, home = %home.display(), "cannot stat the trash data home");
                None
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, home = %home.display(), "cannot open the trash data home");
            None
        }
    }
}

fn unusable_as_unavailable<T>(result: Result<T, Errno>, what: &str) -> Result<T, FsError> {
    result.map_err(|e| {
        tracing::warn!(error = %e, "{what} is not usable");
        FsError::TrashUnavailable
    })
}

/// Climbs `..` while the device stays the same; the last directory on the file's device is the mount point.
fn find_mount_point(
    parent: &OwnedFd,
    parent_path: Option<&Path>,
    dev: u64,
) -> Result<(OwnedFd, Option<PathBuf>), Errno> {
    let mut cur = openat(parent, ".", DIR_FLAGS, Mode::empty())?;
    let mut path = parent_path.map(Path::to_path_buf);
    loop {
        let cur_st = fstat(&cur)?;
        let up = openat(&cur, "..", DIR_FLAGS, Mode::empty())?;
        let up_st = fstat(&up)?;
        let at_top =
            up_st.st_dev != dev || (up_st.st_dev, up_st.st_ino) == (cur_st.st_dev, cur_st.st_ino);
        if at_top {
            return Ok((cur, path));
        }
        cur = up;
        path = path.and_then(|p| p.parent().map(Path::to_path_buf));
    }
}

fn trash_dirs(base: &OwnedFd, name: &str, topdir: Option<PathBuf>) -> Result<TrashDirs, Errno> {
    let trash = open_or_make_dir(base, name)?;
    Ok(TrashDirs {
        files: open_or_make_dir(&trash, "files")?,
        info: open_or_make_dir(&trash, "info")?,
        topdir,
    })
}

/// An existing entry must be a real directory (never a symlink) owned by us, so another user cannot
/// plant a trash we would move files into.
fn open_or_make_dir(parent: &OwnedFd, name: &str) -> Result<OwnedFd, Errno> {
    match mkdirat(parent, name, Mode::from_raw_mode(DIR_MODE)) {
        Ok(()) | Err(Errno::EXIST) => {}
        Err(e) => return Err(e),
    }
    let fd = openat(parent, name, DIR_FLAGS, Mode::empty())?;
    if fstat(&fd)?.st_uid != geteuid().as_raw() {
        return Err(Errno::PERM);
    }
    Ok(fd)
}

fn move_into(
    parent: &OwnedFd,
    leaf: &str,
    original: &Path,
    date: &str,
    dirs: &TrashDirs,
) -> Result<(), FsError> {
    let contents = info_contents(original, dirs.topdir.as_deref(), date);
    let base = truncate_to_boundary(leaf, MAX_TRASH_NAME_BYTES);
    for attempt in 1..=MAX_NAME_ATTEMPTS {
        let trash_name = if attempt == 1 {
            base.to_owned()
        } else {
            format!("{base}.{attempt}")
        };
        let info_name = format!("{trash_name}.trashinfo");
        match write_info(&dirs.info, &info_name, &contents) {
            Ok(()) => {}
            Err(WriteInfo::Taken) => continue,
            Err(WriteInfo::Failed(e)) => return Err(e),
        }
        let renamed = renameat_with(
            parent,
            leaf,
            &dirs.files,
            trash_name.as_str(),
            RenameFlags::NOREPLACE,
        );
        match renamed {
            Ok(()) => return Ok(()),
            Err(e) => {
                remove_own_info(&dirs.info, &info_name);
                match e {
                    Errno::EXIST => continue,
                    Errno::XDEV => return Err(FsError::TrashUnavailable),
                    other => return Err(map_errno(other)),
                }
            }
        }
    }
    Err(FsError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no free name in the trash",
    )))
}

enum WriteInfo {
    Taken,
    Failed(FsError),
}

fn write_info(info: &OwnedFd, name: &str, contents: &str) -> Result<(), WriteInfo> {
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = match openat(info, name, flags, Mode::from_raw_mode(INFO_MODE)) {
        Ok(fd) => fd,
        Err(Errno::EXIST) => return Err(WriteInfo::Taken),
        Err(e) => return Err(WriteInfo::Failed(map_errno(e))),
    };
    if let Err(e) = File::from(fd).write_all(contents.as_bytes()) {
        remove_own_info(info, name);
        return Err(WriteInfo::Failed(FsError::Io(e)));
    }
    Ok(())
}

/// Removes the `.trashinfo` this call created for a move that did not happen. It is the daemon's own
/// bookkeeping file; the user's file is never unlinked.
fn remove_own_info(info: &OwnedFd, name: &str) {
    if let Err(e) = unlinkat(info.as_fd(), name, AtFlags::empty()) {
        tracing::error!(error = %e, name, "cannot remove the unused trash info file");
    }
}

fn info_contents(original: &Path, topdir: Option<&Path>, date: &str) -> String {
    let shown = topdir
        .and_then(|top| original.strip_prefix(top).ok())
        .unwrap_or(original);
    format!(
        "[Trash Info]\nPath={}\nDeletionDate={date}\n",
        percent_encode(shown.as_os_str())
    )
}

fn percent_encode(path: &OsStr) -> String {
    let mut out = String::new();
    for &b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn truncate_to_boundary(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
