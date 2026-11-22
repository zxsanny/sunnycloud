//! AZ-3860 failure paths through the real runner binary and the host's real ffmpeg: the stall kill of a
//! real encode (SIGSTOP), the wall limit, cancel, a clean non-zero encoder exit, outputs that fail
//! validation, an unknown `params_version`, and the argv every child receives. The tools that must
//! misbehave are shell stubs ahead of the real ones on the runner's `PATH`; everything else is real.
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use common::media::{
    MediaRunner, RunnerEnd, WAIT, cpu_plan, events_until_response, live_descendants,
    path_with_scripts, probe, real_tool, recorded_calls, recording_script, transcode, video_limits,
};
use common::video_ref::Clips;
use media_shared::decoder_protocol::{DecodeError, LimitKind, Request, Response};
use media_shared::policy::{AudioPlan, Encoder, Plan};
use tempfile::TempDir;

/// A clip whose encode takes long enough to be interrupted: noisy 1080p, one thread.
fn long_clip(clips: &Clips) -> Vec<u8> {
    fs::read(clips.noisy(1920, 1080, 8)).expect("clip")
}

fn one_thread() -> media_shared::decoder_protocol::Limits {
    let mut limits = video_limits();
    limits.cpu = 1;
    limits
}

fn ffmpeg_pid(runner: &MediaRunner) -> u32 {
    let deadline = Instant::now() + WAIT;
    loop {
        let found = live_descendants(runner.pid()).into_iter().find(|pid| {
            fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|cmdline| String::from_utf8_lossy(&cmdline).contains("result.mp4"))
        });
        if let Some(pid) = found {
            return pid;
        }
        assert!(Instant::now() < deadline, "no ffmpeg child appeared");
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_progress_block(job: &Path) {
    let sink = job.join("work/child.stdout");
    let deadline = Instant::now() + WAIT;
    while !fs::read_to_string(&sink).is_ok_and(|text| text.contains("progress=")) {
        assert!(Instant::now() < deadline, "ffmpeg wrote no progress block");
        thread::sleep(Duration::from_millis(10));
    }
}

fn gone(pid: u32) -> bool {
    !Path::new(&format!("/proc/{pid}")).exists()
}

/// Milliseconds the guard says it waited for a progress block before the kill, read from the
/// runner's log (the guard's own clock, so a loaded host cannot move it).
fn silent_ms(runner: &MediaRunner) -> u64 {
    let log = runner.log();
    let line = log
        .lines()
        .find(|line| line.contains("Killed(Stall)"))
        .unwrap_or_else(|| panic!("no stall kill in the log: {log}"));
    let after = line
        .split("last progress ")
        .nth(1)
        .and_then(|rest| rest.split(" ms before the end").next())
        .unwrap_or_else(|| panic!("no silence in {line}"));
    after
        .trim_start_matches("Some(")
        .trim_end_matches(')')
        .parse()
        .unwrap_or_else(|_| panic!("not a number: {after}"))
}

#[test]
fn a_real_encode_stopped_mid_run_is_killed_as_stalled_after_the_stall_timeout_and_the_runner_restarts()
 {
    let clips = Clips::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&long_clip(&clips));
    let mut limits = one_thread();
    limits.stall_timeout = 3000;
    client.send(Request::Transcode {
        job_dir: job.to_str().expect("utf8").into(),
        plan: cpu_plan(false, AudioPlan::None),
        limits,
    });
    wait_for_progress_block(&job);
    let pid = ffmpeg_pid(&runner);
    let stopped = Command::new("kill")
        .args(["-STOP", &pid.to_string()])
        .status()
        .expect("run kill");
    assert!(stopped.success(), "SIGSTOP was not delivered");

    let (response, _) = events_until_response(&mut client, WAIT * 3);

    assert_eq!(response, Response::Error(DecodeError::Stalled));
    let silent = silent_ms(&runner);
    assert!(
        (3000..13_000).contains(&silent),
        "killed {silent} ms after the last block"
    );
    assert!(gone(pid), "the stopped ffmpeg is still there");
    assert!(!job.join("work").exists());
    assert_eq!(
        runner.end_within(WAIT),
        RunnerEnd::Exited(3),
        "a stall is hostile-class"
    );
}

#[test]
fn a_real_encode_past_the_wall_limit_is_killed_as_a_limit_and_the_runner_restarts() {
    let clips = Clips::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&long_clip(&clips));
    let mut limits = one_thread();
    limits.wall = 1500;

    let started = Instant::now();
    let (response, _) = transcode(&mut client, &job, cpu_plan(false, AudioPlan::None), limits);

    assert_eq!(
        response,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Wall
        })
    );
    assert!(started.elapsed() >= Duration::from_millis(1500));
    assert!(!job.join("work").exists());
    assert_eq!(runner.end_within(WAIT), RunnerEnd::Exited(3));
}

#[test]
fn a_cancel_mid_encode_kills_ffmpeg_and_the_runner_keeps_serving() {
    let clips = Clips::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&long_clip(&clips));
    client.send(Request::Transcode {
        job_dir: job.to_str().expect("utf8").into(),
        plan: cpu_plan(false, AudioPlan::None),
        limits: one_thread(),
    });
    wait_for_progress_block(&job);
    let pid = ffmpeg_pid(&runner);

    client.send(Request::Cancel);
    let (response, _) = events_until_response(&mut client, WAIT);

    assert_eq!(response, Response::Cancelled);
    assert!(gone(pid));
    assert!(!job.join("work").exists());
    assert_eq!(
        runner.end_within(Duration::from_millis(300)),
        RunnerEnd::StillRunning
    );
    let again = runner.job(&fs::read(clips.path("noisy-1920x1080-8.mp4")).expect("clip"));
    assert!(matches!(probe(&mut client, &again), Response::Probe(_)));
}

fn stubbed_runner(ffmpeg: &str) -> (TempDir, MediaRunner) {
    let (bin, path) = path_with_scripts(&[("ffmpeg", ffmpeg)]);
    (bin, MediaRunner::start_with(&[], &path))
}

#[test]
fn a_clean_non_zero_encoder_exit_is_encoder_failed_with_its_code_and_the_runner_keeps_serving() {
    let clips = Clips::new();
    let clip = fs::read(clips.sdr("a.mp4", 320, 240, 1, true)).expect("clip");
    let (_bin, mut runner) = stubbed_runner("exit 7");
    let mut client = runner.connect();
    let job = runner.job(&clip);

    let (response, _) = transcode(
        &mut client,
        &job,
        cpu_plan(false, AudioPlan::Aac),
        video_limits(),
    );

    assert_eq!(
        response,
        Response::Error(DecodeError::EncoderFailed { exit_code: 7 })
    );
    assert_eq!(
        runner.end_within(Duration::from_millis(300)),
        RunnerEnd::StillRunning
    );
    let next = runner.job(&clip);
    assert!(matches!(probe(&mut client, &next), Response::Probe(_)));
    let (remux, _) = transcode(&mut client, &next, Plan::Remux, video_limits());
    assert_eq!(
        remux,
        Response::Error(DecodeError::EncoderFailed { exit_code: 7 })
    );
}

/// A stub `ffmpeg` that ignores its arguments and writes `result.mp4` (in its working directory
/// `out/`) with the real ffmpeg from `source_args`.
fn writing_stub(source_args: &str) -> String {
    format!(
        "exec {} -loglevel error -y {source_args} -f mp4 result.mp4",
        real_tool("ffmpeg").display()
    )
}

#[test]
fn an_output_that_fails_validation_is_output_invalid_and_the_runner_keeps_serving() {
    let clips = Clips::new();
    let clip = fs::read(clips.sdr("four.mp4", 320, 240, 4, true)).expect("clip");
    let cases = [
        (
            "no video stream",
            writing_stub("-f lavfi -i sine=duration=4 -c:a aac"),
        ),
        (
            "three seconds short",
            writing_stub("-f lavfi -i testsrc2=s=320x240:r=10:d=1 -c:v libx264 -pix_fmt yuv420p"),
        ),
        (
            "not H.264",
            writing_stub("-f lavfi -i testsrc2=s=320x240:r=10:d=4 -c:v libx265"),
        ),
        ("no output file", "exit 0".to_owned()),
    ];

    for (name, stub) in cases {
        let (_bin, mut runner) = stubbed_runner(&stub);
        let mut client = runner.connect();
        let job = runner.job(&clip);

        let (response, _) = transcode(
            &mut client,
            &job,
            cpu_plan(false, AudioPlan::None),
            video_limits(),
        );

        assert_eq!(
            response,
            Response::Error(DecodeError::OutputInvalid),
            "{name}"
        );
        assert_eq!(
            runner.end_within(Duration::from_millis(300)),
            RunnerEnd::StillRunning,
            "{name}"
        );
    }
}

#[test]
fn every_child_gets_the_whitelists_the_first_streams_only_and_the_clamped_thread_cap() {
    let clips = Clips::new();
    let clip = fs::read(clips.sdr("a.mp4", 640, 360, 1, true)).expect("clip");
    let logs = TempDir::new().expect("log dir");
    let (ffmpeg_log, ffprobe_log) = (
        logs.path().join("ffmpeg.log"),
        logs.path().join("ffprobe.log"),
    );
    let (ffmpeg, ffprobe) = (
        recording_script("ffmpeg", &ffmpeg_log),
        recording_script("ffprobe", &ffprobe_log),
    );
    let (_bin, path) = path_with_scripts(&[("ffmpeg", &ffmpeg), ("ffprobe", &ffprobe)]);
    let mut runner = MediaRunner::start_with(&[], &path);
    let mut client = runner.connect();
    let job = runner.job(&clip);
    let mut greedy = video_limits();
    greedy.cpu = 64;

    let (response, _) = transcode(&mut client, &job, cpu_plan(false, AudioPlan::Aac), greedy);

    assert!(
        matches!(response, Response::TranscodeResult(_)),
        "{response:?}"
    );
    let value_after = |call: &[String], flag: &str| {
        call.iter()
            .position(|line| line == &format!("arg {flag}"))
            .map(|at| call[at + 1].clone())
    };
    let has = |call: &[String], flag: &str| call.iter().any(|line| line == &format!("arg {flag}"));
    let ffmpeg_calls = recorded_calls(&ffmpeg_log);
    assert_eq!(ffmpeg_calls.len(), 1);
    for call in &ffmpeg_calls {
        assert!(has(call, "-nostdin"));
        assert_eq!(
            value_after(call, "-protocol_whitelist").as_deref(),
            Some("arg file")
        );
        assert!(has(call, "-format_whitelist") && has(call, "-codec_whitelist"));
        assert_eq!(
            value_after(call, "-map_metadata").as_deref(),
            Some("arg -1")
        );
        assert_eq!(value_after(call, "-map").as_deref(), Some("arg 0:v:0"));
        assert!(has(call, "0:a:0?"));
        assert_eq!(value_after(call, "-threads").as_deref(), Some("arg 4"));
        assert_eq!(
            value_after(call, "-filter_threads").as_deref(),
            Some("arg 4")
        );
        assert_eq!(
            value_after(call, "-progress").as_deref(),
            Some("arg pipe:1")
        );
        assert!(
            call.iter()
                .filter(|line| line.starts_with("arg "))
                .all(|line| !line.contains(job.to_str().expect("utf8")))
        );
        assert!(call[0].ends_with("/in/source"), "{}", call[0]);
    }
    let probes = recorded_calls(&ffprobe_log);
    assert_eq!(probes.len(), 2, "the input and the output are probed");
    assert!(probes[0][0].ends_with("/in/source") && probes[1][0].ends_with("/out/result.mp4"));
    for call in &probes {
        assert_eq!(
            value_after(call, "-protocol_whitelist").as_deref(),
            Some("arg file")
        );
        assert!(has(call, "-format_whitelist") && has(call, "-codec_whitelist"));
    }
}

fn photo(clips: &Clips) -> PathBuf {
    let png = clips.path("p.png");
    common::media::run_tool(
        "ffmpeg",
        &[
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=64x64",
            "-frames:v",
            "1",
            png.to_str().expect("utf8"),
        ],
    );
    png
}

#[test]
fn an_unknown_params_version_is_a_protocol_mismatch_for_a_still_and_a_plan_and_the_runner_keeps_serving()
 {
    let clips = Clips::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let video = runner.job(&fs::read(clips.sdr("a.mp4", 320, 240, 1, true)).expect("clip"));
    let still = runner.job(&fs::read(photo(&clips)).expect("png"));
    let dir = |job: &Path| job.to_str().expect("utf8").to_owned();
    let render = |job: &Path, params_version| Request::RenderStill {
        job_dir: dir(job),
        max_edge: 32,
        poster_at_ms: None,
        params_version,
        limits: common::media::limits(),
    };
    let plan = Plan::Transcode {
        params_version: 2,
        encoder: Encoder::Cpu,
        scale_to_short_edge: 1080,
        tone_map: false,
        audio: AudioPlan::Aac,
        faststart: true,
    };

    client.send(render(&still, 2));
    let stale_still = client.expect_response(WAIT);
    client.send(render(&video, 99));
    let stale_poster = client.expect_response(WAIT);
    let (stale_plan, _) = transcode(&mut client, &video, plan, video_limits());
    client.send(render(&still, 1));
    let known = client.expect_response(WAIT);

    for response in [stale_still, stale_poster, stale_plan] {
        assert_eq!(response, Response::Error(DecodeError::ProtocolMismatch));
    }
    assert!(matches!(known, Response::ImageResult(_)), "{known:?}");
    assert_eq!(
        runner.end_within(Duration::from_millis(300)),
        RunnerEnd::StillRunning
    );
}

#[test]
fn the_gpu_plan_and_a_non_video_input_are_unsupported_until_the_nvenc_task_exists() {
    let clips = Clips::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let video = runner.job(&fs::read(clips.sdr("a.mp4", 320, 240, 1, true)).expect("clip"));
    let still = runner.job(&fs::read(photo(&clips)).expect("png"));
    let nvenc = Plan::Transcode {
        params_version: 1,
        encoder: Encoder::Nvenc,
        scale_to_short_edge: 1080,
        tone_map: false,
        audio: AudioPlan::Aac,
        faststart: true,
    };

    let (gpu, _) = transcode(&mut client, &video, nvenc, video_limits());
    let (png, _) = transcode(
        &mut client,
        &still,
        cpu_plan(false, AudioPlan::Aac),
        video_limits(),
    );
    let (png_remux, _) = transcode(&mut client, &still, Plan::Remux, video_limits());

    for response in [gpu, png, png_remux] {
        assert_eq!(response, Response::Error(DecodeError::Unsupported));
    }
    assert_eq!(
        runner.end_within(Duration::from_millis(300)),
        RunnerEnd::StillRunning
    );
}
