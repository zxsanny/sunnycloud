//! The kills and limits of AZ-3854 with the numbers of its acceptance criteria. A kill is timed
//! against the instants the guard records (`Finished::timing`), never against when the test
//! thread notices the return, so a loaded machine cannot move the measurement.

use std::fs;
use std::hint::black_box;

use super::*;

const SECOND: Duration = Duration::from_secs(1);

/// What a sh child prints as one ffmpeg progress block.
const BLOCK_FN: &str =
    "b() { printf 'out_time_us=%s\\nspeed=1.0x\\nprogress=continue\\n' \"$1\"; }";

fn limits_with_stall(wall: Duration, stall: Duration) -> JobLimits {
    JobLimits {
        stall_timeout: stall,
        ..limits(wall, 1 << 30, 1 << 30)
    }
}

fn since(later: Instant, earlier: Instant) -> Duration {
    later.duration_since(earlier)
}

/// The progress events the guard forwarded so far, as `(out_time_ms, speed)`.
fn events_of(harness: &Harness) -> Vec<(u64, f64)> {
    harness
        .messages
        .try_iter()
        .filter_map(|message| match message {
            JobMessage::Progress { out_time_ms, speed } => Some((out_time_ms, speed)),
            JobMessage::Done(_) => None,
        })
        .collect()
}

pub(super) fn wait_until_group_is_empty(pgid: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !group_is_empty(pgid) {
        assert!(Instant::now() < deadline, "the group is still alive");
        thread::yield_now();
    }
}

fn leader_of(finished: &Finished) -> u32 {
    stdout_of(finished)
        .lines()
        .next()
        .and_then(|line| line.trim().parse().ok())
        .expect("the child's pid on its first line")
}

#[test]
fn a_child_that_sleeps_is_killed_at_the_wall_limit_within_one_second_with_its_whole_group() {
    let harness = harness(b"");
    let limits = limits(Duration::from_secs(3), 1 << 30, 1 << 30);

    let finished = run_child(
        &harness,
        "/bin/sh",
        &["-c", "echo $$; sleep 30 & sleep 30 & wait"],
        &[],
        &limits,
    );

    assert_eq!(finished.outcome, Outcome::Killed(Kill::Wall));
    let acted = since(finished.timing.decided, finished.timing.started);
    assert!(acted >= Duration::from_secs(3), "{acted:?}");
    assert!(acted < Duration::from_secs(4), "{acted:?}");
    wait_until_group_is_empty(leader_of(&finished));
}

/// The child of the memory test. It is started by that test, with the same test binary, because
/// a child that reports when it crossed the limit needs a program of its own; run directly it
/// would hold 512 MiB for 30 s.
#[test]
#[ignore = "the child of a guard test: started by that test, never on its own"]
fn child_grows_past_the_memory_limit_and_reports_when() {
    let mut held: Vec<Vec<u8>> = vec![vec![1u8; 100 << 20]];
    eprintln!("CROSSING_AT {}", uptime_seconds());
    for _ in 0..26 {
        held.push(vec![1u8; 16 << 20]);
    }
    black_box(&held);
    thread::sleep(Duration::from_secs(30));
}

/// `/proc/uptime` has a resolution of 10 ms; the test calibrates it against `Instant` once.
fn uptime_seconds() -> f64 {
    fs::read_to_string("/proc/uptime")
        .expect("read /proc/uptime")
        .split_whitespace()
        .next()
        .and_then(|text| text.parse().ok())
        .expect("uptime in seconds")
}

fn this_test_binary() -> String {
    std::env::current_exe()
        .expect("the path of the test binary")
        .to_str()
        .expect("utf8 path")
        .to_owned()
}

#[test]
fn a_child_allocating_512_mib_against_a_128_mib_limit_is_killed_within_one_second_of_crossing() {
    let harness = harness(b"");
    let limits = limits(Duration::from_secs(30), 128 << 20, 1 << 30);
    let child = "process::tests::kills::child_grows_past_the_memory_limit_and_reports_when";

    let finished = run_with(
        &harness,
        &this_test_binary(),
        &["--ignored", "--exact", child, "--nocapture"],
        &[],
        &limits,
        Duration::from_millis(100),
        false,
    );

    assert_eq!(finished.outcome, Outcome::Killed(Kill::Memory));
    let stamp: f64 = finished
        .stderr_tail
        .split_whitespace()
        .skip_while(|word| *word != "CROSSING_AT")
        .nth(1)
        .and_then(|text| text.trim_end_matches('?').parse().ok())
        .unwrap_or_else(|| panic!("the child reported no time: {}", finished.stderr_tail));
    let (now, uptime_now) = (Instant::now(), uptime_seconds());
    // The child reports at 100 MiB, a few megabytes below the limit; the guard polls every 100 ms.
    let reported_at = now - Duration::from_secs_f64(uptime_now - stamp);
    let lag = since(finished.timing.decided, reported_at);
    assert!(lag < SECOND, "{lag:?}");
}

#[test]
fn a_child_writing_a_gibibyte_is_stopped_by_the_file_size_limit_at_one_mebibyte() {
    let harness = harness(b"");
    let limits = limits(Duration::from_secs(30), 1 << 30, 1 << 20);

    let finished = run_child(
        &harness,
        "/bin/dd",
        &["if=/dev/zero", "of=big", "bs=1M", "count=1024"],
        &[],
        &limits,
    );

    assert_eq!(finished.outcome, Outcome::Signaled(25));
    let written = fs::metadata(harness.dir.path().join("big")).expect("the output file");
    assert_eq!(written.len(), 1 << 20);
}

#[test]
fn a_mebibyte_of_stderr_with_escapes_leaves_a_sanitised_tail_of_at_most_two_kibibytes() {
    let harness = harness(b"");
    let limits = limits(Duration::from_secs(3), 1 << 30, 1 << 30);
    let script = "yes \"$(printf '\\033[31mred\\nline')\" | head -c 1048576 >&2; sleep 30";

    let finished = run_child(&harness, "/bin/sh", &["-c", script], &[], &limits);

    assert_eq!(finished.outcome, Outcome::Killed(Kill::Wall));
    let stderr_file = fs::metadata(harness.dir.path().join(STDERR_FILE)).expect("stderr file");
    assert!(stderr_file.len() >= 1 << 20, "{}", stderr_file.len());
    assert!(finished.stderr_tail.len() <= 2048);
    assert!(
        finished.stderr_tail.contains('?'),
        "the escapes are replaced"
    );
    assert!(
        finished
            .stderr_tail
            .chars()
            .all(|c| (' '..='~').contains(&c))
    );
}

#[test]
fn a_child_that_prints_progress_and_stops_is_killed_as_stalled_within_a_second_of_the_stall_timeout()
 {
    let harness = harness(b"");
    let limits = limits_with_stall(Duration::from_secs(60), Duration::from_secs(3));
    let script = format!("{BLOCK_FN}; echo $$; sleep 30 & b 1000000; sleep 1; b 2000000; wait");

    let finished = run_with(
        &harness,
        "/bin/sh",
        &["-c", &script],
        &[],
        &limits,
        Duration::from_millis(20),
        true,
    );

    assert_eq!(finished.outcome, Outcome::Killed(Kill::Stall));
    let last = finished.timing.last_progress.expect("two blocks were seen");
    let silent_for = since(finished.timing.decided, last);
    assert!(silent_for >= Duration::from_secs(3), "{silent_for:?}");
    assert!(silent_for < Duration::from_secs(4), "{silent_for:?}");
    assert_eq!(events_of(&harness), [(1000, 1.0), (2000, 1.0)]);
    wait_until_group_is_empty(leader_of(&finished));
}

#[test]
fn a_child_that_keeps_printing_a_block_every_second_for_ten_seconds_is_never_stall_killed() {
    let harness = harness(b"");
    let limits = limits_with_stall(Duration::from_secs(60), Duration::from_secs(3));
    let script = format!(
        "{BLOCK_FN}; i=1; while [ $i -le 10 ]; do b $((i * 1000000)); sleep 1; i=$((i + 1)); done"
    );

    let finished = run_with(
        &harness,
        "/bin/sh",
        &["-c", &script],
        &[],
        &limits,
        Duration::from_millis(20),
        true,
    );

    assert_eq!(finished.outcome, Outcome::Exited(0));
    let times: Vec<u64> = events_of(&harness).iter().map(|event| event.0).collect();
    assert_eq!(
        times,
        [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10_000]
    );
}

#[test]
fn a_child_that_never_writes_a_block_is_stalled_from_its_start() {
    let harness = harness(b"");
    let limits = limits_with_stall(Duration::from_secs(60), SECOND);

    let finished = run_with(
        &harness,
        "/bin/sleep",
        &["30"],
        &[],
        &limits,
        Duration::from_millis(20),
        true,
    );

    assert_eq!(finished.outcome, Outcome::Killed(Kill::Stall));
    assert!(finished.timing.last_progress.is_none());
    let silent_for = since(finished.timing.decided, finished.timing.started);
    assert!(silent_for >= SECOND, "{silent_for:?}");
    assert!(silent_for < Duration::from_secs(2), "{silent_for:?}");
}

#[test]
fn a_tool_without_a_progress_sink_is_never_stall_killed() {
    let harness = harness(b"");
    let limits = limits_with_stall(Duration::from_secs(60), Duration::from_millis(300));

    let finished = run_child(&harness, "/bin/sleep", &["1"], &[], &limits);

    assert_eq!(finished.outcome, Outcome::Exited(0));
}
