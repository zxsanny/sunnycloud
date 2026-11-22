//! `ProcessGuard` (E-19/c06): every media child starts from an argv vector with a cleared
//! environment, no stdin, its own process group and fixed rlimits. The guard kills the group on
//! the wall limit, on memory above the limit and on cancel, and returns a typed outcome.
//! Nothing keys on the child's output text.
//!
//! The child's stdout and stderr go to two files in `work/`, so the guard has no reader thread
//! that a descendant holding a pipe open could keep waiting. An ffmpeg child started with
//! `-progress pipe:1` and `Spec::progress` writes its progress blocks to the stdout file; the
//! guard reads them as the file grows, hands each to the job as a `Progress` event and kills
//! the group when no block arrives for `stall_timeout` (10 section 4, gate S5).

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
use crate::progress::ProgressReader;
use crate::sys::{self, ChildLimits};

const WATCH_TICK: Duration = Duration::from_millis(5);
/// A child that prints blocks faster than this is not forwarded faster: the channel to the
/// connection is unbounded and the child is untrusted.
const PROGRESS_FORWARD_EVERY: Duration = Duration::from_millis(100);
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
    /// No progress block for `stall_timeout`.
    Stall,
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
    pub timing: Timing,
}

/// When the guard acted, in its own clock: a kill is timed against these instants and not
/// against when a caller notices it, so a loaded machine cannot move the measurement.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub started: Instant,
    /// The last progress block the guard saw; `None` when the child never wrote one.
    pub last_progress: Option<Instant>,
    /// When the guard decided how the child ended (a kill is issued at this instant).
    pub decided: Instant,
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
    /// The child writes ffmpeg progress blocks to its stdout (`-progress pipe:1`): they become
    /// `Progress` events and their absence for `limits.stall_timeout` is a `Kill::Stall`.
    pub progress: bool,
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
    let (outcome, timing) = match watch(&mut child, spec, context, &stdout_path) {
        Ok(watched) => watched,
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
        timing,
        stdout: read_head(&stdout_path, STDOUT_CAP)?,
        stderr_tail: read_tail(&stderr_path, STDERR_TAIL)?,
    })
}

fn watch(
    child: &mut Child,
    spec: &Spec<'_>,
    context: &JobContext,
    stdout_path: &Path,
) -> io::Result<(Outcome, Timing)> {
    let pgid = child.id();
    let started = Instant::now();
    let mut timing = Timing {
        started,
        last_progress: None,
        decided: started,
    };
    let mut sink = if spec.progress {
        Some(ProgressReader::open(stdout_path)?)
    } else {
        None
    };
    let mut last_alive = started;
    let mut last_forward: Option<Instant> = None;
    let mut next_memory_check = started + spec.memory_poll;
    let outcome = loop {
        if let Some(status) = child.try_wait()? {
            break classify(status);
        }
        let now = Instant::now();
        if let Some(sink) = sink.as_mut() {
            for block in sink.poll()? {
                last_alive = now;
                timing.last_progress = Some(now);
                let due =
                    last_forward.is_none_or(|at| now.duration_since(at) >= PROGRESS_FORWARD_EVERY);
                if let (true, Some(out_time_ms)) = (due, block.out_time_ms) {
                    last_forward = Some(now);
                    context.progress(out_time_ms, block.speed);
                }
            }
            if now.duration_since(last_alive) >= spec.limits.stall_timeout {
                break Outcome::Killed(Kill::Stall);
            }
        }
        if context.is_cancelled() {
            break Outcome::Killed(Kill::Cancelled);
        }
        if now.duration_since(started) >= spec.limits.wall {
            break Outcome::Killed(Kill::Wall);
        }
        if now >= next_memory_check {
            next_memory_check = now + spec.memory_poll;
            if group_rss(pgid)? > spec.limits.memory_bytes {
                break Outcome::Killed(Kill::Memory);
            }
        }
        thread::sleep(WATCH_TICK);
    };
    timing.decided = Instant::now();
    Ok((outcome, timing))
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
