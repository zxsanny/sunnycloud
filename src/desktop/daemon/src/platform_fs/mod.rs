//! The one file-system trait the daemon reads and writes through.
//!
//! Every `name` is a [`RelPath`]-shaped string (relative, `/`-separated, no `.`/`..`) resolved beneath a
//! directory handle; no path string ever reaches a write. Implementations: `linux::LinuxFs` (Linux),
//! and `fake::FakeFs` for unit tests of other modules.

mod error;
#[cfg(test)]
pub mod fake;
#[cfg(target_os = "linux")]
pub mod linux;
mod rel;

use std::io;
use std::path::Path;

pub use error::FsError;
pub use rel::RelPath;
use sync_core::FileSource;

/// Prefix of every temp file made by `create_new_in`, so scans and cleanup can recognise them.
pub const TEMP_PREFIX: &str = ".sunnycloud-tmp-";

/// Device and inode (volume and file index on Windows). Stable across renames on one volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId {
    pub volume: u64,
    pub index: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Fifo,
    Socket,
    Device,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub kind: EntryKind,
    pub size: u64,
    /// Nanoseconds since the Unix epoch, saturating outside the `i64` range.
    pub mtime_ns: i64,
    pub id: FileId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub stat: FileStat,
}

/// The entries of one directory (not recursive; descend with `open_dir`).
#[derive(Debug, Default)]
pub struct DirListing {
    pub entries: Vec<DirEntry>,
    /// Entries whose name is not valid UTF-8; they are not listed and cannot be synced.
    pub unrepresentable: u64,
}

/// A freshly created, exclusively owned temp file next to its future target.
pub trait TempFile: Send + Sync {
    fn name(&self) -> &str;

    fn write_all_at(&self, offset: u64, bytes: &[u8]) -> io::Result<()>;
}

pub trait PlatformFs: Send + Sync {
    /// A directory handle. All operations resolve their `name` beneath one.
    type Dir: Send + Sync;
    type Temp: TempFile;
    type Source: FileSource + 'static;

    /// Opens the sync root. The root path is user-chosen and may itself go through symlinks.
    fn open_root(&self, path: &Path) -> Result<Self::Dir, FsError>;

    /// A handle for the directory `name` beneath `dir`; a symlink anywhere on the way is `EscapesRoot`.
    fn open_dir(&self, dir: &Self::Dir, name: &str) -> Result<Self::Dir, FsError>;

    /// Probe: create a lower-case random temp file in `root`, open it by its upper-cased name, delete it.
    fn is_case_insensitive(&self, root: &Self::Dir) -> Result<bool, FsError>;

    /// Creates, exclusively, a temp file named `TEMP_PREFIX` + random in the directory of `name`
    /// (the future target). Never truncates or reuses an existing file.
    fn create_new_in(&self, dir: &Self::Dir, name: &str) -> Result<Self::Temp, FsError>;

    /// Flushes `temp` and atomically renames it over `name`, which must be in the temp file's directory.
    /// The target shows the old or the new content at every moment.
    fn rename_over_in(&self, dir: &Self::Dir, temp: Self::Temp, name: &str) -> Result<(), FsError>;

    /// Removes a temp file that will not be renamed (the only unlink this trait performs).
    fn discard_temp(&self, dir: &Self::Dir, temp: Self::Temp) -> Result<(), FsError>;

    /// Moves `name` to the OS trash. Never unlinks: with no usable trash it returns `TrashUnavailable`
    /// and the file stays.
    fn trash(&self, dir: &Self::Dir, name: &str) -> Result<(), FsError>;

    fn stat_no_follow(&self, dir: &Self::Dir, name: &str) -> Result<FileStat, FsError>;

    fn file_id(&self, dir: &Self::Dir, name: &str) -> Result<FileId, FsError> {
        Ok(self.stat_no_follow(dir, name)?.id)
    }

    fn is_placeholder(&self, dir: &Self::Dir, name: &str) -> Result<bool, FsError>;

    /// Opens a regular file for shared read. Anything else (symlink, FIFO, directory) is refused.
    fn open_for_read(&self, dir: &Self::Dir, name: &str) -> Result<Self::Source, FsError>;

    fn walk(&self, dir: &Self::Dir) -> Result<DirListing, FsError>;
}
