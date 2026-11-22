//! The progress sink and the stall kill with the real `ffmpeg` of the host: AZ-3849 AC-2 in
//! miniature (progress blocks arrive under the protocol whitelist) and IT-10-13 at host level
//! (a stopped ffmpeg is a `Stall`, a running one is not).

use std::fs;
use std::process::Command;
use std::time::SystemTime;

use super::kills::wait_until_group_is_empty;
use super::*;

const WHITELISTS: [&str; 6] = [
    "-protocol_whitelist",
    "file",
    "-format_whitelist",
    "mov,matroska,avi,mpegts",
    "-codec_whitelist",
    "h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt",
];

/// A `seconds` long H.264 clip made by the host's ffmpeg.
fn clip(seconds: u32) -> Vec<u8> {
    let dir = TempDir::new().expect("clip dir");
    let path = dir.path().join("clip.mp4");
    let source = format!("testsrc=d={seconds}:s=160x120:r=10");
    let status = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i", &source])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&path)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg could not make the clip");
    fs::read(&path).expect("read the clip")
}

/// The decoder's ffmpeg options with the progress sink of gate S5, reading the input at its own
/// pace (`-re`) so that the run lasts as long as the clip. `marker` makes the process findable.
fn realtime_args(marker: &str) -> Vec<String> {
    let mut args: Vec<String> = ["-nostdin", "-nostats", "-loglevel", "error"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    args.extend(WHITELISTS.map(str::to_owned));
    args.extend(
        ["-progress", "pipe:1", "-re", "-i", &input_path()]
            .into_iter()
            .map(str::to_owned),
    );
    args.extend(["-metadata", marker, "-f", "null", "-"].map(str::to_owned));
    args
}

fn run_ffmpeg(harness: &Harness, args: &[String], limits: &JobLimits) -> Finished {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    run_with(
        harness,
        "/usr/bin/ffmpeg",
        &args,
        &[],
        limits,
        Duration::from_millis(100),
        true,
    )
}

fn limits_for(stall: Duration) -> JobLimits {
    JobLimits {
        stall_timeout: stall,
        ..limits(Duration::from_secs(120), 1 << 30, 1 << 30)
    }
}

fn pid_with_argument(argument: &str) -> Option<u32> {
    fs::read_dir("/proc")
        .expect("read /proc")
        .filter_map(Result::ok)
        .find_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let cmdline = fs::read(entry.path().join("cmdline")).ok()?;
            String::from_utf8_lossy(&cmdline)
                .contains(argument)
                .then_some(pid)
        })
}

fn unique_marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("guardtest={}-{nanos}", std::process::id())
}

#[test]
fn a_real_ffmpeg_under_the_file_whitelist_writes_progress_blocks_every_half_second() {
    let harness = harness(&clip(6));
    let args = realtime_args(&unique_marker());

    let finished = run_ffmpeg(&harness, &args, &limits_for(Duration::from_secs(3)));

    // A run that finished means no gap of 3 s ever separated two blocks.
    assert_eq!(
        finished.outcome,
        Outcome::Exited(0),
        "{}",
        finished.stderr_tail
    );
    let seconds = since_start(&finished);
    let blocks = stdout_of(&finished)
        .lines()
        .filter(|line| line.starts_with("progress="))
        .count();
    eprintln!(
        "RECORDED real ffmpeg under -protocol_whitelist file: {blocks} blocks in {seconds:.1} s"
    );
    assert!(
        blocks as f64 >= seconds.floor(),
        "{blocks} blocks in {seconds} s"
    );
    let events = events_of_harness(&harness);
    assert!(events.len() as f64 >= seconds.floor(), "{events:?}");
    assert!(
        events.windows(2).all(|pair| pair[0] <= pair[1]),
        "{events:?}"
    );
}

fn since_start(finished: &Finished) -> f64 {
    finished
        .timing
        .decided
        .duration_since(finished.timing.started)
        .as_secs_f64()
}

fn events_of_harness(harness: &Harness) -> Vec<u64> {
    harness
        .messages
        .try_iter()
        .filter_map(|message| match message {
            JobMessage::Progress { out_time_ms, .. } => Some(out_time_ms),
            JobMessage::Done(_) => None,
        })
        .collect()
}

#[test]
fn a_real_ffmpeg_stopped_mid_run_is_killed_as_stalled_with_its_group_and_one_that_runs_is_not() {
    let harness = harness(&clip(30));
    let marker = unique_marker();
    let args = realtime_args(&marker);
    let sink = harness.dir.path().join(STDOUT_FILE);

    let (finished, stopped) = thread::scope(|scope| {
        let stopper = scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !fs::read_to_string(&sink).is_ok_and(|text| text.contains("progress=")) {
                assert!(Instant::now() < deadline, "ffmpeg wrote no progress block");
                thread::sleep(Duration::from_millis(10));
            }
            let pid = pid_with_argument(&marker).expect("the ffmpeg process");
            let status = Command::new("kill")
                .args(["-STOP", &pid.to_string()])
                .status()
                .expect("run kill");
            assert!(status.success(), "SIGSTOP was not delivered");
            pid
        });
        let finished = run_ffmpeg(&harness, &args, &limits_for(Duration::from_secs(3)));
        (finished, stopper.join().expect("the stopper thread"))
    });

    assert_eq!(finished.outcome, Outcome::Killed(Kill::Stall));
    let last = finished.timing.last_progress.expect("blocks were seen");
    let silent_for = finished.timing.decided.duration_since(last);
    assert!(silent_for >= Duration::from_secs(3), "{silent_for:?}");
    assert!(silent_for < Duration::from_secs(4), "{silent_for:?}");
    wait_until_group_is_empty(stopped);
}
