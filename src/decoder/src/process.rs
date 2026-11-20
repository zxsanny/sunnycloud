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
    // A normal end can leave descendants in the group; the group is swept either way.
    end_group(&mut child);
    Ok(Finished {
        outcome,
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
mod tests {
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;

    use tempfile::TempDir;

    use super::*;
    use crate::proc_table::snapshot;

    struct Harness {
        dir: TempDir,
        input: File,
        cancel: Arc<AtomicBool>,
        context: JobContext,
    }

    fn harness(input_bytes: &[u8]) -> Harness {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("source");
        File::create(&path)
            .and_then(|mut file| file.write_all(input_bytes))
            .expect("write input");
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, _messages) = channel();
        let context = JobContext::new(
            File::open(&path).expect("open input"),
            Arc::clone(&cancel),
            sender,
            Arc::new(AtomicBool::new(false)),
        );
        Harness {
            input: File::open(&path).expect("open input"),
            dir,
            cancel,
            context,
        }
    }

    fn limits(wall: Duration, memory_bytes: u64, max_output_bytes: u64) -> JobLimits {
        JobLimits {
            cpu: 1,
            memory_bytes,
            wall,
            stall_timeout: Duration::from_secs(90),
            max_output_bytes,
        }
    }

    fn run_child(
        harness: &Harness,
        program: &str,
        args: &[&str],
        env: &[(&'static str, &str)],
        limits: &JobLimits,
    ) -> Finished {
        let spec = Spec {
            program: Path::new(program),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: env.iter().map(|(k, v)| (*k, (*v).to_owned())).collect(),
            cwd: harness.dir.path(),
            work: harness.dir.path(),
            input: &harness.input,
            limits,
            memory_poll: Duration::from_millis(20),
        };
        run(&spec, &harness.context).expect("guard run")
    }

    fn roomy() -> JobLimits {
        limits(Duration::from_secs(30), 1 << 30, 1 << 30)
    }

    fn stdout_of(finished: &Finished) -> String {
        String::from_utf8(finished.stdout.clone()).expect("utf8")
    }

    fn group_is_empty(pgid: u32) -> bool {
        !snapshot()
            .expect("snapshot")
            .iter()
            .any(|process| process.pgrp == pgid)
    }

    #[test]
    fn a_hostile_argument_reaches_the_child_as_one_argument_and_no_shell_runs() {
        let harness = harness(b"");
        let hostile = "dir with spaces; $(touch pwned) `id` 'quote\" && x";

        let finished = run_child(&harness, "/bin/echo", &["-n", hostile], &[], &roomy());

        assert_eq!(finished.outcome, Outcome::Exited(0));
        assert_eq!(stdout_of(&finished), hostile);
        assert!(!harness.dir.path().join("pwned").exists());
    }

    #[test]
    fn the_child_environment_is_exactly_the_allowlist_and_stdin_is_empty() {
        let harness = harness(b"");

        let environment = run_child(
            &harness,
            "/usr/bin/env",
            &[],
            &[("ALPHA", "1"), ("BETA", "two words")],
            &roomy(),
        );
        let stdin = run_child(&harness, "/bin/cat", &[], &[], &roomy());

        let mut lines: Vec<String> = stdout_of(&environment).lines().map(str::to_owned).collect();
        lines.sort();
        assert_eq!(lines, ["ALPHA=1", "BETA=two words"]);
        assert_eq!(stdin.outcome, Outcome::Exited(0));
        assert!(stdin.stdout.is_empty());
    }

    #[test]
    fn the_input_is_the_fixed_descriptor_path_and_cannot_be_named_by_the_job() {
        let harness = harness(b"the input bytes");

        let finished = run_child(&harness, "/bin/cat", &[&input_path()], &[], &roomy());

        assert_eq!(finished.outcome, Outcome::Exited(0));
        assert_eq!(stdout_of(&finished), "the input bytes");
    }

    #[test]
    fn the_rlimits_are_set_and_no_address_space_limit_is_applied() {
        let harness = harness(b"");
        let limits = limits(Duration::from_secs(30), 1 << 30, 3 << 20);

        let finished = run_child(&harness, "/bin/cat", &["/proc/self/limits"], &[], &limits);

        let table = stdout_of(&finished);
        let row = |name: &str| {
            table
                .lines()
                .find(|line| line.starts_with(name))
                .unwrap_or_else(|| panic!("{name} missing in {table}"))
                .split_whitespace()
                .rev()
                .nth(2)
                .map(str::to_owned)
                .expect("soft limit column")
        };
        assert_eq!(row("Max core file size"), "0");
        assert_eq!(row("Max file size"), (3u64 << 20).to_string());
        assert_eq!(row("Max open files"), OPEN_FILES.to_string());
        assert_eq!(row("Max address space"), "unlimited");
        assert_eq!(row("Max cpu time"), "35");
    }

    #[test]
    fn a_child_that_sleeps_is_killed_at_the_wall_limit_with_its_whole_group() {
        let harness = harness(b"");
        let limits = limits(Duration::from_millis(400), 1 << 30, 1 << 30);
        let started = Instant::now();

        let finished = run_child(
            &harness,
            "/bin/sh",
            &["-c", "echo $$; sleep 30 & sleep 30 & wait"],
            &[],
            &limits,
        );

        assert_eq!(finished.outcome, Outcome::Killed(Kill::Wall));
        assert!(
            started.elapsed() < Duration::from_millis(1400),
            "{:?}",
            started.elapsed()
        );
        let pgid: u32 = stdout_of(&finished)
            .trim()
            .parse()
            .expect("the child's pid");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !group_is_empty(pgid) {
            assert!(Instant::now() < deadline, "the group is still alive");
            thread::yield_now();
        }
    }

    #[test]
    fn a_child_above_the_memory_limit_is_killed_at_the_next_poll() {
        let harness = harness(b"");
        let limits = limits(Duration::from_secs(30), 64 << 20, 1 << 30);
        let started = Instant::now();

        // dd fills a 700 MB buffer and then blocks writing to a pipe that nobody reads.
        let finished = run_child(
            &harness,
            "/bin/sh",
            &["-c", "dd if=/dev/zero bs=700M count=1 | sleep 30"],
            &[],
            &limits,
        );

        assert_eq!(finished.outcome, Outcome::Killed(Kill::Memory));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_child_writing_past_the_file_size_limit_ends_by_the_file_size_signal() {
        let harness = harness(b"");
        let limits = limits(Duration::from_secs(30), 1 << 30, 1 << 20);

        let finished = run_child(&harness, "/bin/cat", &["/dev/zero"], &[], &limits);

        assert_eq!(finished.outcome, Outcome::Signaled(25));
    }

    #[test]
    fn exit_code_and_signal_are_typed_whatever_stderr_says() {
        let harness = harness(b"");

        let exited = run_child(
            &harness,
            "/bin/sh",
            &["-c", "echo Killed >&2; exit 1"],
            &[],
            &roomy(),
        );
        let crashed = run_child(&harness, "/bin/sh", &["-c", "kill -SEGV $$"], &[], &roomy());

        assert_eq!(exited.outcome, Outcome::Exited(1));
        assert_eq!(crashed.outcome, Outcome::Signaled(11));
    }

    #[test]
    fn stderr_is_kept_as_a_two_kilobyte_tail_without_control_characters() {
        let harness = harness(b"");
        let script = "i=0; while [ $i -lt 20000 ]; do printf '\\033[31mred\\nline\\r\\n' >&2; i=$((i+1)); done; exit 3";

        let finished = run_child(&harness, "/bin/sh", &["-c", script], &[], &roomy());

        assert_eq!(finished.outcome, Outcome::Exited(3));
        assert!(finished.stderr_tail.len() <= 2048);
        assert!(!finished.stderr_tail.is_empty());
        assert!(
            finished
                .stderr_tail
                .chars()
                .all(|c| (' '..='~').contains(&c))
        );
    }

    #[test]
    fn a_cancelled_job_kills_its_child_at_once() {
        let harness = harness(b"");
        harness.cancel.store(true, Ordering::Release);
        let started = Instant::now();

        let finished = run_child(&harness, "/bin/sleep", &["30"], &[], &roomy());

        assert_eq!(finished.outcome, Outcome::Killed(Kill::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
