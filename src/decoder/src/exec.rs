//! What one job needs to start children: the tools, the limits, the directories and the job's
//! `work/`, which is removed on every way out of the job.

use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use media_shared::decoder_protocol::DecodeError;
use sc_observability::log_error;

use crate::failure::{Failure, check};
use crate::job::JobContext;
use crate::limits::JobLimits;
use crate::process::{self, Finished, Spec};
use crate::tools::Tools;

const WORK: &str = "work";
const OUT: &str = "out";

/// `job_dir/work`, empty at creation and removed on drop.
pub(crate) struct Work {
    path: PathBuf,
}

impl Work {
    pub(crate) fn create(job_dir: &Path) -> io::Result<Self> {
        let path = job_dir.join(WORK);
        // Left behind by an earlier job that the runner did not outlive; `remove_dir_all` does
        // not follow a symlink, it removes the link.
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(&path)?,
            Ok(_) => fs::remove_file(&path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            log_error!("cannot remove work/ after a job", err = &error);
        }
    }
}

/// The settings of the executors that come from the runner's config rather than from the job.
#[derive(Debug, Clone, Copy)]
pub struct ExecutorConfig {
    pub photo_max_pixels: u64,
    pub memory_poll: Duration,
}

pub(crate) struct Exec<'a> {
    pub tools: &'a Tools,
    pub config: &'a ExecutorConfig,
    pub limits: &'a JobLimits,
    pub context: &'a JobContext,
    pub job_dir: &'a Path,
    pub work: &'a Work,
}

impl Exec<'_> {
    pub(crate) fn out_dir(&self) -> PathBuf {
        self.job_dir.join(OUT)
    }

    /// `out/` as a real directory: a symlink there would send the output elsewhere.
    pub(crate) fn checked_out_dir(&self) -> Result<PathBuf, Failure> {
        let out = self.out_dir();
        match fs::symlink_metadata(&out) {
            Ok(meta) if meta.is_dir() => Ok(out),
            Ok(_) => Err(DecodeError::Corrupt.into()),
            Err(error) => Err(internal("cannot inspect out/", &error)),
        }
    }

    /// Runs one child with `input` as its `/proc/self/fd/9` and returns it when it ended with
    /// exit code 0; any other end is the typed failure.
    pub(crate) fn run(
        &self,
        tool: &'static str,
        program: &Path,
        args: Vec<String>,
        env: Vec<(&'static str, String)>,
        cwd: &Path,
        input: &File,
    ) -> Result<Finished, Failure> {
        let spec = Spec {
            program,
            args,
            env,
            cwd,
            work: self.work.path(),
            input,
            limits: self.limits,
            memory_poll: self.config.memory_poll,
        };
        let finished = process::run(&spec, self.context)
            .map_err(|error| internal("cannot run a media tool", &error))?;
        check(tool, &finished)?;
        Ok(finished)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{what}: {cause}")]
struct Fault {
    what: &'static str,
    cause: String,
}

/// A fault of the runner's own environment (a tool that will not start, a directory that
/// cannot be made) is not the file's fault; it is logged in full and answered as an invalid
/// output, the only typed answer that does not blame the input.
pub(crate) fn internal(what: &'static str, source: &io::Error) -> Failure {
    log_error!(
        "job failed inside the runner",
        err = &Fault {
            what,
            cause: source.to_string(),
        }
    );
    DecodeError::OutputInvalid.into()
}
