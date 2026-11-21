//! `ProcessGuard` (E-19/c06): every media child starts from an argv vector with a cleared
//! environment, no stdin, its own process group and fixed rlimits. The guard kills the group on
//! the wall limit, on memory above the limit and on cancel, and returns a typed outcome.
//! Nothing keys on the child's output text.
//!
//! The child's stdout and stderr go to two files in `work/`, so the guard has no reader thread
//! that a descendant holding a pipe open could keep waiting. The stall kill belongs to the
//! ffmpeg progress sink and arrives with the video transcode.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sc_observability::log_warn;

use crate::job::JobContext;
use crate::limits::JobLimits;
use crate::proc_table::{group_alive, group_rss};
use crate::sys::{self, ChildLimits};

const WATCH_TICK: Duration = Duration::from_millis(5);
const OPEN_FILES: u64 = 256;
const GROUP_END_LIMIT: Duration = Duration::from_secs(2);
/// A header read prints a few lines and an ffprobe report a few KiB; the rest is cut.
const STDOUT_CAP: u64 = 1 << 20;
const STDERR_TAIL: u64 = 2048;
const STDOUT_FILE: &str = "child.stdout";
const STDERR_FILE: &str = "child.stderr";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kill {
    Wall,
    Memory,
    Cancelled,
}

/// How a child ended. A child that left through a signal is not an exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Exited(i32),
    Signaled(i32),
    Killed(Kill),
}

#[derive(Debug)]
pub struct Finished {
    pub outcome: Outcome,
    pub stdout: Vec<u8>,
    /// The child ended by itself but left a live process in its own process group. No media tool
    /// does that, so the group is killed and the job is hostile-class.
    pub stragglers: bool,
    /// At most 2 KiB of the end of stderr with every control character replaced; for logs only.
    pub stderr_tail: String,
}

pub struct Spec<'a> {
    pub program: &'a Path,
    pub args: Vec<String>,
    /// The whole environment of the child: nothing else is inherited.
    pub env: Vec<(&'static str, String)>,
    /// The directory the child starts in; the output names in `args` are relative to it.
    pub cwd: &'a Path,
    /// A directory the guard may write the child's stdout and stderr files into.
    pub work: &'a Path,
    /// Reachable by the child as `/proc/self/fd/9` ([`input_path`]).
    pub input: &'a File,
    pub limits: &'a JobLimits,
    pub memory_poll: Duration,
}

/// The path a child reads the job input from.
pub fn input_path() -> String {
    format!("/proc/self/fd/{}", sys::INPUT_FD)
}

/// Runs the child to its end or to the kill. By the time this returns no process of the child's
/// group is left.
pub fn run(spec: &Spec<'_>, context: &JobContext) -> io::Result<Finished> {
    let stdout_path = spec.work.join(STDOUT_FILE);
    let stderr_path = spec.work.join(STDERR_FILE);
    let mut command = Command::new(spec.program);
    command
        .args(spec.args.iter().map(OsStr::new))
        .env_clear()
        .envs(spec.env.iter().map(|(key, value)| (key, value)))
        .current_dir(spec.cwd)
        .stdin(Stdio::null())
        .stdout(File::create(&stdout_path)?)
        .stderr(File::create(&stderr_path)?)
        .process_group(0);
    sys::confine(
        &mut command,
        ChildLimits {
            max_file_bytes: spec.limits.max_output_bytes,
            cpu_seconds: spec
                .limits
                .wall
                .as_secs()
                .saturating_mul(u64::from(spec.limits.cpu.max(1)))
                .saturating_add(5),
            open_files: OPEN_FILES,
        },
        spec.input.as_raw_fd(),
    );
    let mut child = command.spawn()?;
    let outcome = match watch(&mut child, spec, context) {
        Ok(outcome) => outcome,
        Err(error) => {
            end_group(&mut child);
            return Err(error);
        }
    };
    // Decided before the sweep: the leader has gone, so a member still alive was left behind.
    let stragglers = match left_behind(&outcome, child.id()) {
        Ok(left) => left,
        Err(error) => {
            end_group(&mut child);
            return Err(error);
        }
    };
    end_group(&mut child);
    Ok(Finished {
        outcome,
        stragglers,
        stdout: read_head(&stdout_path, STDOUT_CAP)?,
        stderr_tail: read_tail(&stderr_path, STDERR_TAIL)?,
    })
}

fn watch(child: &mut Child, spec: &Spec<'_>, context: &JobContext) -> io::Result<Outcome> {
    let pgid = child.id();
    let started = Instant::now();
    let mut next_memory_check = started + spec.memory_poll;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(classify(status));
        }
        if context.is_cancelled() {
            return Ok(Outcome::Killed(Kill::Cancelled));
        }
        let now = Instant::now();
        if now.duration_since(started) >= spec.limits.wall {
            return Ok(Outcome::Killed(Kill::Wall));
        }
        if now >= next_memory_check {
            next_memory_check = now + spec.memory_poll;
            if group_rss(pgid)? > spec.limits.memory_bytes {
                return Ok(Outcome::Killed(Kill::Memory));
            }
        }
        thread::sleep(WATCH_TICK);
    }
}

/// A kill ends the group on purpose; any other end of the leader should leave nobody behind.
fn left_behind(outcome: &Outcome, pgid: u32) -> io::Result<bool> {
    match outcome {
        Outcome::Killed(_) => Ok(false),
        Outcome::Exited(_) | Outcome::Signaled(_) => group_alive(pgid),
    }
}

fn classify(status: ExitStatus) -> Outcome {
    match (status.code(), status.signal()) {
        (Some(code), _) => Outcome::Exited(code),
        (None, Some(signal)) => Outcome::Signaled(signal),
        (None, None) => Outcome::Signaled(0),
    }
}

/// SIGKILLs the whole group, then reaps the leader. Killing first keeps the group id reserved
/// by the unreaped leader. When the child already ended by itself the kill only sweeps members
/// that outlived it; an empty group answers `ESRCH`, which is not an error.
fn end_group(child: &mut Child) {
    if let Err(error) = sys::kill_group(child.id()) {
        log_warn!("cannot kill a media child's process group", err = &error);
    }
    if let Err(error) = child.wait() {
        log_warn!("cannot reap a media child", err = &error);
    }
    wait_until_group_ended(child.id());
}

/// SIGKILL is delivered at once but a process leaves the table a moment later. Waiting here
/// means that a live process found afterwards is one that escaped the group.
fn wait_until_group_ended(pgid: u32) {
    let deadline = Instant::now() + GROUP_END_LIMIT;
    loop {
        match group_alive(pgid) {
            Ok(false) => return,
            Ok(true) if Instant::now() < deadline => thread::yield_now(),
            Ok(true) => {
                log_warn!("a killed process group is still alive");
                return;
            }
            Err(error) => {
                log_warn!("cannot read the process table", err = &error);
                return;
            }
        }
    }
}

fn read_head(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(cap).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_tail(path: &Path, cap: u64) -> io::Result<String> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(cap)))?;
    let mut bytes = Vec::new();
    file.take(cap).read_to_end(&mut bytes)?;
    Ok(bytes
        .iter()
        .map(|&byte| {
            if (0x20..0x7f).contains(&byte) {
                char::from(byte)
            } else {
                '?'
            }
        })
        .collect())
}

#[cfg(test)]
mod tests;
