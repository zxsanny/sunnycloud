//! IT-10-12 (stub): requested limits are clamped to the runner's ceilings and the guard enforces them.
//! The runner has `photo_job_timeout=3s` and `photo_memory=128MiB`; each request asks for far more.

use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{DecodeError, LimitKind, Response};

use crate::common::media::Media;
use crate::decoder::{Decoder, Flavor, stub_image};
use crate::marker::Behaviour;
use crate::peer::{greedy_limits, render, render_at};
use crate::refs::{pattern_jpeg, workdir};

const CEILINGS: [(&str, &str); 2] = [
    ("SUNNYCLOUD_PHOTO_JOB_TIMEOUT", "3s"),
    ("SUNNYCLOUD_PHOTO_MEMORY", "128MiB"),
];
const WAIT: Duration = Duration::from_secs(30);

/// Runs one marked job on a fresh container, waits for the hostile-class restart and checks the
/// group-end state (AC-7). Returns the answer and how long it took.
fn marked(behaviour: &Behaviour) -> (Response, Duration) {
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &CEILINGS);
    let before = decoder.state();
    let job = decoder.job(&behaviour.jpeg());
    let mut peer = decoder.connect();
    let started = Instant::now();

    let answer = peer.call(render(&job, 320, greedy_limits()), WAIT);
    let elapsed = started.elapsed();

    decoder.wait_restarted(&before, WAIT).hello();
    decoder.finish();
    (answer, elapsed)
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_12_a_hung_tool_is_killed_at_the_clamped_wall_not_the_requested_one() {
    let (answer, elapsed) = marked(&Behaviour::Hang);

    assert_eq!(
        answer,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Wall
        })
    );
    assert!(
        (Duration::from_millis(2900)..=Duration::from_secs(4)).contains(&elapsed),
        "3 s ceiling, killed after {elapsed:?}"
    );
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_12_an_allocation_is_killed_at_the_clamped_memory_threshold() {
    let (answer, elapsed) = marked(&Behaviour::Alloc(512));

    assert_eq!(
        answer,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Memory
        })
    );
    assert!(elapsed < Duration::from_secs(2), "killed after {elapsed:?}");
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_12_a_1_gib_output_is_stopped_by_the_file_size_limit() {
    let (answer, _) = marked(&Behaviour::Write(1024));

    assert_eq!(
        answer,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Output
        })
    );
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_12_a_tool_spinning_on_all_cores_is_killed_at_the_wall() {
    let (answer, elapsed) = marked(&Behaviour::Spin);

    assert_eq!(
        answer,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Wall
        })
    );
    assert!(
        elapsed <= Duration::from_secs(4),
        "killed after {elapsed:?}"
    );
}

fn argv_of<'a>(lines: &'a [serde_json::Value], tool: &str) -> Vec<&'a serde_json::Value> {
    lines.iter().filter(|line| line["tool"] == tool).collect()
}

fn env_value(line: &serde_json::Value, key: &str) -> Option<String> {
    line["env"]
        .as_array()?
        .iter()
        .find_map(|pair| (pair[0] == key).then(|| pair[1].as_str().unwrap_or_default().to_owned()))
}

fn limit_row(line: &serde_json::Value, row: &str) -> String {
    line["limits"]
        .as_str()
        .and_then(|text| text.lines().find(|candidate| candidate.starts_with(row)))
        .unwrap_or_else(|| panic!("no {row} in the limits of {line}"))
        .to_owned()
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_12_real_jobs_run_with_the_clamped_thread_count_and_the_documented_rlimits() {
    let work = workdir();
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &CEILINGS);
    let photo = decoder.job(&pattern_jpeg(work.path(), "p.jpg", 1200, 900));
    let clip = decoder.job(&Media::new().mp4(2));
    let mut peer = decoder.connect();

    let still = peer.call(render(&photo, 320, greedy_limits()), WAIT);
    let poster = peer.call(render_at(&clip, 320, 500), WAIT);

    assert!(matches!(still, Response::ImageResult(_)), "{still:?}");
    assert!(matches!(poster, Response::ImageResult(_)), "{poster:?}");
    let photo_log = decoder.stub_log(&photo);
    let thumbnails = argv_of(&photo_log, "vips");
    assert_eq!(thumbnails.len(), 1, "{photo_log:?}");
    assert_eq!(
        env_value(thumbnails[0], "VIPS_CONCURRENCY").as_deref(),
        Some("2"),
        "cpu 64 clamps to the ceiling 2"
    );
    let clip_log = decoder.stub_log(&clip);
    let frames = argv_of(&clip_log, "ffmpeg");
    assert_eq!(frames.len(), 1, "{clip_log:?}");
    let argv: Vec<&str> = frames[0]["argv"]
        .as_array()
        .expect("argv")
        .iter()
        .filter_map(|a| a.as_str())
        .collect();
    for flag in ["-threads", "-filter_threads"] {
        let at = argv
            .iter()
            .position(|arg| *arg == flag)
            .unwrap_or_else(|| panic!("no {flag} in {argv:?}"));
        assert_eq!(argv[at + 1], "2", "{flag} equals the clamped cpu");
    }
    for line in photo_log.iter().chain(&clip_log) {
        let text = line["argv"].to_string();
        assert!(
            !text.contains("--linear") && !text.contains("unlimited"),
            "{text}"
        );
        assert!(
            limit_row(line, "Max address space").contains("unlimited"),
            "no RLIMIT_AS"
        );
        let core = limit_row(line, "Max core file size");
        assert!(
            core.split_whitespace()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .starts_with(&["bytes", "0", "0"]),
            "{core}"
        );
    }
    let file_size = limit_row(&photo_log[0], "Max file size");
    assert!(
        file_size.contains("33554432"),
        "RLIMIT_FSIZE is the 32 MiB output ceiling: {file_size}"
    );
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_12_a_tool_that_ignores_sigterm_is_killed_at_the_wall() {
    let (answer, elapsed) = marked(&Behaviour::IgnoreTerm);

    assert_eq!(
        answer,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Wall
        })
    );
    assert!(
        elapsed <= Duration::from_secs(4),
        "killed after {elapsed:?}"
    );
}

/// ST-10-4: a tool that writes 1 MiB of stderr (with a fake secret and escape sequences) and is then
/// killed leaves at most a 2 KiB sanitised tail in the runner's log.
#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn st_10_4_one_mib_of_stderr_leaves_a_short_sanitised_tail_in_the_log() {
    let mut decoder = Decoder::start(
        &stub_image(),
        Flavor::Interactive,
        &[("SUNNYCLOUD_PHOTO_JOB_TIMEOUT", "2s")],
    );
    let before = decoder.state();
    let job = decoder.job(&Behaviour::Stderr(1024).jpeg());
    let mut peer = decoder.connect();

    let answer = peer.call(render(&job, 320, greedy_limits()), WAIT);

    assert_eq!(
        answer,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Wall
        })
    );
    let logs = decoder.logs();
    let failed: Vec<&str> = logs
        .lines()
        .filter(|line| line.contains("media tool failed"))
        .collect();
    assert_eq!(failed.len(), 1, "{logs}");
    assert!(
        failed[0].len() <= 2048 + 512,
        "the log line is {} bytes",
        failed[0].len()
    );
    assert!(
        !failed[0].contains('\u{1b}') && !failed[0].contains('\r'),
        "control characters in the log"
    );
    assert!(
        !logs.contains(&job.dir()),
        "the job directory is in the log"
    );
    decoder.wait_restarted(&before, WAIT).hello();
    decoder.finish();
}
