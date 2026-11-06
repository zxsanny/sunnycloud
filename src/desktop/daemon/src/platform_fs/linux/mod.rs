//! Linux [`PlatformFs`]: handle-relative system calls only (see `resolve`), freedesktop trash (see `trash`).

mod local_time;
mod resolve;
mod source;
mod trash;

use std::fs::File;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, Stat, renameat, statat, unlinkat};
use rustix::rand::{GetRandomFlags, getrandom};
use sc_names::FsProfile;

use self::resolve::{Resolver, map_errno};
pub use self::source::{LinuxSource, LinuxTemp};
use super::{
    DirEntry, DirListing, EntryKind, FileId, FileStat, FsError, PlatformFs, RelPath, TEMP_PREFIX,
};

const PROFILE: FsProfile = FsProfile::PosixCaseSensitive;
const FILE_MODE: u32 = 0o666;
const PROBE_PREFIX: &str = "sc-probe-";
const NAME_ATTEMPTS: u32 = 8;

/// A directory handle plus the path it was reached by, kept only to write `Path=` in `.trashinfo`.
#[derive(Debug)]
pub struct LinuxDir {
    fd: OwnedFd,
    path: PathBuf,
}

#[derive(Debug)]
pub struct LinuxFs {
    resolver: Resolver,
    data_home: Option<PathBuf>,
}

impl Default for LinuxFs {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxFs {
    /// Uses `openat2` when the kernel (and its sandbox) allows it, otherwise per-component `O_NOFOLLOW`.
    /// The home trash is `$XDG_DATA_HOME/Trash`, or `$HOME/.local/share/Trash`.
    pub fn new() -> Self {
        Self {
            resolver: Resolver::detect(),
            data_home: data_home_from_env(),
        }
    }

    /// Forces the per-component fallback used on kernels without `openat2`.
    pub fn without_openat2(mut self) -> Self {
        self.resolver = Resolver::component_walk();
        self
    }

    pub fn with_data_home(mut self, data_home: PathBuf) -> Self {
        self.data_home = Some(data_home);
        self
    }

    pub fn uses_openat2(&self) -> bool {
        self.resolver.uses_openat2()
    }
}

fn data_home_from_env() -> Option<PathBuf> {
    let absolute = |v: std::ffi::OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    std::env::var_os("XDG_DATA_HOME")
        .and_then(absolute)
        .or_else(|| {
            std::env::var_os("HOME")
                .and_then(absolute)
                .map(|home| home.join(".local/share"))
        })
}

impl PlatformFs for LinuxFs {
    type Dir = LinuxDir;
    type Temp = LinuxTemp;
    type Source = LinuxSource;

    fn open_root(&self, path: &Path) -> Result<LinuxDir, FsError> {
        let path = path.canonicalize()?;
        let fd = rustix::fs::openat(
            CWD,
            &path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(map_errno)?;
        Ok(LinuxDir { fd, path })
    }

    fn open_dir(&self, dir: &LinuxDir, name: &str) -> Result<LinuxDir, FsError> {
        let rel = RelPath::parse(name, PROFILE)?;
        let fd = self.resolver.open(
            dir.fd.as_fd(),
            &rel,
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )?;
        Ok(LinuxDir {
            fd,
            path: dir.path.join(rel.as_string()),
        })
    }

    fn is_case_insensitive(&self, root: &LinuxDir) -> Result<bool, FsError> {
        let lower = format!("{PROBE_PREFIX}{}", random_hex()?);
        let upper = RelPath::parse(&lower.to_uppercase(), PROFILE)?;
        let lower = RelPath::parse(&lower, PROFILE)?;
        let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL;
        self.resolver.open(
            root.fd.as_fd(),
            &lower,
            flags,
            Mode::from_raw_mode(FILE_MODE),
        )?;
        let found = match self
            .resolver
            .open(root.fd.as_fd(), &upper, OFlags::RDONLY, Mode::empty())
        {
            Ok(_) => Ok(true),
            Err(e) if e.is_not_found() => Ok(false),
            Err(e) => Err(e),
        };
        unlinkat(root.fd.as_fd(), lower.leaf(), AtFlags::empty()).map_err(map_errno)?;
        found
    }

    fn create_new_in(&self, dir: &LinuxDir, name: &str) -> Result<LinuxTemp, FsError> {
        let target_dir = RelPath::parse(name, PROFILE)?.parent();
        let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL;
        for _ in 0..NAME_ATTEMPTS {
            let temp_name = format!("{TEMP_PREFIX}{}", random_hex()?);
            let path = target_dir.join(&temp_name);
            match self
                .resolver
                .open(dir.fd.as_fd(), &path, flags, Mode::from_raw_mode(FILE_MODE))
            {
                Ok(fd) => {
                    return Ok(LinuxTemp {
                        file: File::from(fd),
                        dir: target_dir,
                        name: temp_name,
                    });
                }
                Err(FsError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(FsError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no free temp name",
        )))
    }

    fn rename_over_in(&self, dir: &LinuxDir, temp: LinuxTemp, name: &str) -> Result<(), FsError> {
        let rel = RelPath::parse(name, PROFILE)?;
        if rel.parent() != temp.dir {
            return Err(FsError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the target is not in the temp file's directory",
            )));
        }
        temp.file.sync_all()?;
        let parent = self.resolver.open_parent(dir.fd.as_fd(), &rel)?;
        renameat(&parent, temp.name.as_str(), &parent, rel.leaf()).map_err(map_errno)
    }

    fn discard_temp(&self, dir: &LinuxDir, temp: LinuxTemp) -> Result<(), FsError> {
        let path = temp.dir.join(&temp.name);
        let parent = self.resolver.open_parent(dir.fd.as_fd(), &path)?;
        unlinkat(&parent, temp.name.as_str(), AtFlags::empty()).map_err(map_errno)
    }

    fn trash(&self, dir: &LinuxDir, name: &str) -> Result<(), FsError> {
        let rel = RelPath::parse(name, PROFILE)?;
        trash::trash(
            &self.resolver,
            self.data_home.as_deref(),
            SystemTime::now(),
            dir,
            &rel,
        )
    }

    fn stat_no_follow(&self, dir: &LinuxDir, name: &str) -> Result<FileStat, FsError> {
        let rel = RelPath::parse(name, PROFILE)?;
        let parent = self.resolver.open_parent(dir.fd.as_fd(), &rel)?;
        let stat = statat(&parent, rel.leaf(), AtFlags::SYMLINK_NOFOLLOW).map_err(map_errno)?;
        Ok(file_stat(&stat))
    }

    fn is_placeholder(&self, _dir: &LinuxDir, _name: &str) -> Result<bool, FsError> {
        Ok(false)
    }

    fn open_for_read(&self, dir: &LinuxDir, name: &str) -> Result<LinuxSource, FsError> {
        let rel = RelPath::parse(name, PROFILE)?;
        // NONBLOCK keeps the open of a FIFO from hanging; the type check below then refuses it.
        let fd = self.resolver.open(
            dir.fd.as_fd(),
            &rel,
            OFlags::RDONLY | OFlags::NONBLOCK,
            Mode::empty(),
        )?;
        let file = File::from(fd);
        if !file.metadata()?.is_file() {
            return Err(FsError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            )));
        }
        Ok(LinuxSource::new(file))
    }

    fn walk(&self, dir: &LinuxDir) -> Result<DirListing, FsError> {
        let mut reader = rustix::fs::Dir::read_from(&dir.fd).map_err(map_errno)?;
        let mut listing = DirListing::default();
        while let Some(entry) = reader.read() {
            let entry = entry.map_err(map_errno)?;
            let raw = entry.file_name();
            if matches!(raw.to_bytes(), b"." | b"..") {
                continue;
            }
            let Ok(name) = std::str::from_utf8(raw.to_bytes()) else {
                listing.unrepresentable += 1;
                continue;
            };
            match statat(&dir.fd, raw, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => listing.entries.push(DirEntry {
                    name: name.to_owned(),
                    stat: file_stat(&stat),
                }),
                // Removed between listing and stat: it is gone, not a failure of the walk.
                Err(rustix::io::Errno::NOENT) => {}
                Err(e) => return Err(map_errno(e)),
            }
        }
        Ok(listing)
    }
}

fn file_stat(st: &Stat) -> FileStat {
    let kind = match FileType::from_raw_mode(st.st_mode) {
        FileType::RegularFile => EntryKind::File,
        FileType::Directory => EntryKind::Dir,
        FileType::Symlink => EntryKind::Symlink,
        FileType::Fifo => EntryKind::Fifo,
        FileType::Socket => EntryKind::Socket,
        FileType::BlockDevice | FileType::CharacterDevice => EntryKind::Device,
        FileType::Unknown => EntryKind::Other,
    };
    FileStat {
        kind,
        size: st.st_size.unsigned_abs(),
        mtime_ns: st
            .st_mtime
            .saturating_mul(1_000_000_000)
            .saturating_add(i64::from(st.st_mtime_nsec as i32)),
        id: FileId {
            volume: st.st_dev,
            index: st.st_ino,
        },
    }
}

fn random_hex() -> Result<String, FsError> {
    let mut bytes = [0u8; 8];
    getrandom(&mut bytes, GetRandomFlags::empty()).map_err(map_errno)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests;
