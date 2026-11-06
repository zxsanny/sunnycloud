//! The per-job scratch tree shared with the untrusted decoder (09 section 2, Scratch).
//!
//! `<root>/<job_id>/` holds `in/` (the supervisor writes `source`, mode 0440) and `out/` (the decoder writes
//! `result.<ext>`). The tree is group `decoder_gid` with no world bits, and is removed on every exit path,
//! drop included. Everything the decoder can write is hostile input: see [`ScratchDir::open_output`].

mod resolve;

use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, fchown, lchown};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags, open, statvfs};
use thiserror::Error;
use uuid::Uuid;

pub use resolve::OutputFile;
use resolve::Resolver;

const IN_DIR: &str = "in";
const OUT_DIR: &str = "out";
const SOURCE: &str = "source";

const JOB_DIR_MODE: u32 = 0o750;
const IN_DIR_MODE: u32 = 0o750;
const OUT_DIR_MODE: u32 = 0o770;
const SOURCE_MODE: u32 = 0o440;

/// Why an output file was refused. The decoder's own file names never reach a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RejectReason {
    #[error("a symbolic link")]
    Symlink,
    #[error("missing")]
    Missing,
    #[error("not a regular file")]
    NotRegular,
    #[error("more than one hard link")]
    Hardlinked,
    #[error("larger than the output cap")]
    TooLarge,
    #[error("not reachable beneath the job directory")]
    Unusable,
}

#[derive(Debug, Error)]
pub enum ScratchError {
    /// Permanent failure code `input_too_large`.
    #[error(
        "declared input of {declared} bytes does not fit the {free} bytes free on the scratch mount"
    )]
    InputTooLarge { declared: u64, free: u64 },
    /// Permanent failure code `output_rejected`.
    #[error("decoder output rejected: {0}")]
    OutputRejected(#[from] RejectReason),
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        source: io::Error,
    },
}

impl ScratchError {
    pub(crate) fn io(context: &'static str, source: impl Into<io::Error>) -> Self {
        ScratchError::Io {
            context,
            source: source.into(),
        }
    }
}

/// One scratch volume (`worker_scratch_interactive` or `worker_scratch_general`) and the group the decoder
/// shares with the supervisor.
#[derive(Debug, Clone)]
pub struct ScratchRoot {
    path: PathBuf,
    gid: u32,
    resolver: Resolver,
}

/// Empties every root before the first claim. A supervisor/decoder pair owns its volumes exclusively.
pub fn sweep_all(roots: &[ScratchRoot]) -> Result<(), ScratchError> {
    roots.iter().try_for_each(ScratchRoot::sweep)
}

impl ScratchRoot {
    pub fn new(path: impl Into<PathBuf>, decoder_gid: u32) -> Self {
        Self {
            path: path.into(),
            gid: decoder_gid,
            resolver: Resolver::detect(),
        }
    }

    /// Removes everything inside the root, leaving the root itself.
    pub fn sweep(&self) -> Result<(), ScratchError> {
        let entries = fs::read_dir(&self.path)
            .map_err(|source| ScratchError::io("cannot list the scratch root", source))?;
        for entry in entries {
            let entry =
                entry.map_err(|source| ScratchError::io("cannot list the scratch root", source))?;
            let kind = entry
                .file_type()
                .map_err(|source| ScratchError::io("cannot inspect a leftover entry", source))?;
            remove_entry(&entry.path(), kind.is_dir())
                .map_err(|source| ScratchError::io("cannot remove a leftover entry", source))?;
        }
        Ok(())
    }

    /// Refuses an input larger than the space an unprivileged writer can still use on the mount. Call it before
    /// any byte is fetched.
    pub fn ensure_fits(&self, declared: u64) -> Result<(), ScratchError> {
        let stat = statvfs(&self.path)
            .map_err(|errno| ScratchError::io("cannot read the scratch free space", errno))?;
        let free = stat.f_bavail.saturating_mul(stat.f_frsize);
        if declared > free {
            return Err(ScratchError::InputTooLarge { declared, free });
        }
        Ok(())
    }

    /// Creates `<root>/<job_id>/{in,out}`. A failure part-way removes what was created.
    pub fn create(&self, job_id: Uuid) -> Result<ScratchDir, ScratchError> {
        let path = self.path.join(job_id.to_string());
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|source| {
                ScratchError::io("cannot create the job scratch directory", source)
            })?;
        let dir = ScratchDir {
            path,
            gid: self.gid,
            resolver: self.resolver,
            removed: false,
        };
        dir.prepare()?;
        Ok(dir)
    }
}

/// A job's scratch tree, removed by [`ScratchDir::remove`] or on drop (panic unwinding included).
#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
    gid: u32,
    resolver: Resolver,
    removed: bool,
}

impl ScratchDir {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source_path(&self) -> PathBuf {
        self.path.join(IN_DIR).join(SOURCE)
    }

    /// Creates `in/source` (mode 0440, group `decoder_gid`) and returns it open for writing the fetch.
    pub fn create_source(&self) -> Result<File, ScratchError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(SOURCE_MODE)
            .open(self.source_path())
            .map_err(|source| ScratchError::io("cannot create the input file", source))?;
        file.set_permissions(Permissions::from_mode(SOURCE_MODE))
            .map_err(|source| ScratchError::io("cannot set the input file mode", source))?;
        fchown(&file, None, Some(self.gid))
            .map_err(|source| ScratchError::io("cannot set the input file group", source))?;
        Ok(file)
    }

    /// Empties `out/` between two renders and before a rerun, keeping the directory and its mode.
    pub fn clear_out(&self) -> Result<(), ScratchError> {
        let out = self.path.join(OUT_DIR);
        let entries = fs::read_dir(&out)
            .map_err(|source| ScratchError::io("cannot list the output directory", source))?;
        for entry in entries {
            let entry = entry
                .map_err(|source| ScratchError::io("cannot list the output directory", source))?;
            let kind = entry
                .file_type()
                .map_err(|source| ScratchError::io("cannot inspect an output entry", source))?;
            remove_entry(&entry.path(), kind.is_dir())
                .map_err(|source| ScratchError::io("cannot clear the output directory", source))?;
        }
        Ok(())
    }

    /// Opens `out/result.<ext>` for posting. The name is resolved beneath the job directory without following
    /// any symlink, the file behind it must be regular, have `st_nlink == 1` and at most `cap` bytes, and is
    /// then reopened read-only and non-blocking through the checked handle. Anything else is
    /// [`ScratchError::OutputRejected`], decided before a content byte is read.
    pub fn open_output(&self, ext: &str, cap: u64) -> Result<OutputFile, ScratchError> {
        let job = open(
            &self.path,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|errno| ScratchError::io("cannot open the job directory", errno))?;
        let rel = format!("{OUT_DIR}/result.{ext}");
        resolve::open_output(&self.resolver, job.as_fd(), &rel, cap)
    }

    /// Removes the tree. Calling it again, or on a tree already gone, succeeds.
    pub fn remove(&mut self) -> Result<(), ScratchError> {
        if self.removed {
            return Ok(());
        }
        match fs::remove_dir_all(&self.path) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ScratchError::io(
                    "cannot remove the job scratch tree",
                    source,
                ));
            }
        }
        self.removed = true;
        Ok(())
    }

    fn prepare(&self) -> Result<(), ScratchError> {
        self.set_owner_and_mode(&self.path, JOB_DIR_MODE)?;
        for (name, mode) in [(IN_DIR, IN_DIR_MODE), (OUT_DIR, OUT_DIR_MODE)] {
            let sub = self.path.join(name);
            DirBuilder::new()
                .mode(0o700)
                .create(&sub)
                .map_err(|source| ScratchError::io("cannot create a job subdirectory", source))?;
            self.set_owner_and_mode(&sub, mode)?;
        }
        Ok(())
    }

    fn set_owner_and_mode(&self, dir: &Path, mode: u32) -> Result<(), ScratchError> {
        lchown(dir, None, Some(self.gid))
            .map_err(|source| ScratchError::io("cannot set the job directory group", source))?;
        fs::set_permissions(dir, Permissions::from_mode(mode))
            .map_err(|source| ScratchError::io("cannot set the job directory mode", source))
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if let Err(error) = self.remove() {
            sc_observability::log_error!("job scratch tree not removed", err = &error);
        }
    }
}

/// Removes a file, a symlink (the link, never its target) or a directory tree.
fn remove_entry(path: &Path, is_dir: bool) -> io::Result<()> {
    if is_dir {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod hostile_tests;
