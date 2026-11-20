//! The media tools the runner starts, found once at startup so that a child is always started
//! from an absolute path and the child's own environment needs no `PATH`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::RunnerError;

#[derive(Debug, Clone)]
pub struct Tools {
    pub vipsthumbnail: PathBuf,
    pub vipsheader: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Tools {
    /// Finds each tool in the directories of `path` (the value of `PATH`); a missing tool
    /// refuses the start, because a runner without one would answer every job with an error.
    pub fn locate(path: &str) -> Result<Self, RunnerError> {
        Ok(Self {
            vipsthumbnail: find(path, "vipsthumbnail")?,
            vipsheader: find(path, "vipsheader")?,
            ffmpeg: find(path, "ffmpeg")?,
            ffprobe: find(path, "ffprobe")?,
        })
    }
}

fn find(path: &str, name: &'static str) -> Result<PathBuf, RunnerError> {
    path.split(':')
        .filter(|dir| Path::new(dir).is_absolute())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| is_executable(candidate))
        .ok_or(RunnerError::ToolMissing(name))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_missing_from_every_directory_refuses_the_start() {
        let result = find("/nonexistent:relative/dir", "vipsthumbnail");

        assert!(
            matches!(result, Err(RunnerError::ToolMissing("vipsthumbnail"))),
            "{result:?}"
        );
    }

    #[test]
    fn a_tool_is_found_by_its_absolute_path() {
        let found = find("/nonexistent:/usr/bin:/bin", "sh").expect("sh exists");

        assert!(found.is_absolute() && found.ends_with("sh"));
    }
}
