use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;

use tempfile::TempDir;

use super::*;
use crate::proc_table::snapshot;

/// The killed test children sleep 30 s: a kill that comes before this bound was a kill, not
/// the child ending, however slow the machine.
const KILLED_WELL_BEFORE_THE_CHILD_ENDS: Duration = Duration::from_secs(20);

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

/// No live member: a killed process stays a zombie until its parent reaps it, and where the
/// parent is a container's PID 1 that never reaps (no init) that is for ever.
fn group_is_empty(pgid: u32) -> bool {
    !snapshot()
        .expect("snapshot")
        .iter()
        .any(|process| process.pgrp == pgid && !process.zombie)
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
        started.elapsed() < KILLED_WELL_BEFORE_THE_CHILD_ENDS,
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
fn a_child_that_ends_but_leaves_a_member_of_its_group_alive_is_reported_and_the_member_is_killed() {
    let harness = harness(b"");

    let finished = run_child(
        &harness,
        "/bin/sh",
        &["-c", "sleep 59.771 & echo $!"],
        &[],
        &roomy(),
    );

    assert_eq!(finished.outcome, Outcome::Exited(0));
    assert!(finished.stragglers);
    let straggler: u32 = stdout_of(&finished)
        .trim()
        .parse()
        .expect("the straggler's pid");
    let deadline = Instant::now() + Duration::from_secs(10);
    while snapshot()
        .expect("snapshot")
        .iter()
        .any(|p| p.pid == straggler && !p.zombie)
    {
        assert!(Instant::now() < deadline, "the straggler is still alive");
        thread::yield_now();
    }
}

#[test]
fn a_child_that_ends_alone_leaves_no_straggler() {
    let harness = harness(b"");

    let finished = run_child(&harness, "/bin/echo", &["done"], &[], &roomy());

    assert_eq!(finished.outcome, Outcome::Exited(0));
    assert!(!finished.stragglers);
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
        started.elapsed() < KILLED_WELL_BEFORE_THE_CHILD_ENDS,
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
    assert!(
        started.elapsed() < KILLED_WELL_BEFORE_THE_CHILD_ENDS,
        "{:?}",
        started.elapsed()
    );
}
