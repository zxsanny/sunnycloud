//! IT-10-8 to IT-10-11, IT-10-13 (a) and IT-10-19 with a real transcode, on the release image (real
//! ffmpeg 9.1 with libx264, native `aac` and libzimg). The inputs are clips the harness generates; the
//! references are its own host ffmpeg and ffprobe, never the runner's report and never encoder bytes.
//! What needs the DS corpus, real phone files or a 4K 30 s clip is listed as NOT RUN in the report.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{DecodeError, Request, Response, TranscodeResult};
use media_shared::policy::{AudioPlan, Plan};

use crate::common::video_assert::{assert_compliant, assert_remux};
use crate::common::video_measure::{beep_onsets, bitrate_windows, first_frame, mean_yavg};
use crate::common::video_ref::{Clips, HDR_CHAIN, report_of, streams_of};
use crate::decoder::{Decoder, Flavor, Job, release_image};
use crate::docker::{docker_ok, only_the_runner_runs, processes_of, wait_for};
use crate::peer::{Peer, video_limits};
use crate::refs::{record, ssim};

const WAIT: Duration = Duration::from_secs(180);

fn plan(tone_map: bool, audio: AudioPlan) -> Plan {
    Plan::Transcode {
        params_version: 1,
        encoder: media_shared::policy::Encoder::Cpu,
        scale_to_short_edge: 1080,
        tone_map,
        audio,
        faststart: true,
    }
}

fn request(job: &Job, plan: Plan) -> Request {
    Request::Transcode {
        job_dir: job.dir(),
        plan,
        limits: video_limits(),
    }
}

/// Transcodes `clip` as a job of `decoder` and returns the job, the result and the wall time.
fn run(
    decoder: &mut Decoder,
    peer: &mut Peer,
    clip: &Path,
    plan: Plan,
) -> (Job, TranscodeResult, Duration) {
    let job = decoder.job(&fs::read(clip).expect("clip"));
    let started = Instant::now();
    let answer = peer.call(request(&job, plan), WAIT);
    let wall = started.elapsed();
    let Response::TranscodeResult(result) = answer else {
        panic!("{}: {answer:?}", clip.display())
    };
    (job, result, wall)
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_8_cpu_transcodes_are_compliant_and_a_phone_clip_gets_smaller() {
    let clips = Clips::new();
    let phone = clips.phone_1080p(4, 19_000);
    let uhd = clips.sdr("uhd.mp4", 3840, 2160, 2, true);
    let portrait = clips.sdr("portrait.mp4", 2160, 3840, 2, true);
    let tagged = clips.tagged();
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let mut peer = decoder.connect();

    for (name, clip, expected) in [
        ("1080p phone clip", &phone, (1920, 1080)),
        ("4K SDR", &uhd, (1920, 1080)),
        ("portrait 2160x3840", &portrait, (1080, 1920)),
        ("metadata and subtitle", &tagged, (640, 360)),
    ] {
        let (job, result, wall) = run(&mut decoder, &mut peer, clip, plan(false, AudioPlan::Aac));

        assert_compliant(&job.out("result.mp4"), &result, clip, expected, true);
        let (worst, average) = bitrate_windows(&job.out("result.mp4"));
        let (input, output) = (fs::metadata(clip).expect("input").len(), result.bytes);
        record(
            "IT-10-8",
            &format!(
                "{name}: {}x{} -> {}x{}, input {input} B, output {output} B (ratio {:.3}), video {:.2} Mbit/s average, worst 1 s window {:.2} Mbit, wall {wall:.1?}",
                streams_of(&report_of(clip), "video")[0]["width"],
                streams_of(&report_of(clip), "video")[0]["height"],
                expected.0,
                expected.1,
                output as f64 / input as f64,
                average / 1e6,
                worst as f64 / 1e6,
            ),
        );
        // maxrate 8M with bufsize 2M: no 1 s window carries more than 10 Mbit, nor does the average.
        assert!(average <= 10e6, "{name}: average {average}");
        assert!(worst <= 10_000_000, "{name}: worst window {worst}");
    }
    let (_, result, _) = run(&mut decoder, &mut peer, &phone, plan(false, AudioPlan::Aac));
    assert!(
        result.bytes < fs::metadata(&phone).expect("input").len(),
        "the output of the 1080p phone clip is not smaller than its input"
    );
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_9_rotated_clips_come_out_upright_and_a_variable_rate_clip_keeps_its_beeps_in_place() {
    let clips = Clips::new();
    let work = crate::refs::workdir();
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let mut peer = decoder.connect();

    for degrees in [90, 180, 270] {
        let clip = clips.rotated(&format!("rot{degrees}.mp4"), 640, 360, degrees);
        let expected = if degrees == 180 {
            (640, 360)
        } else {
            (360, 640)
        };
        let (job, result, _) = run(&mut decoder, &mut peer, &clip, plan(false, AudioPlan::Aac));
        assert_compliant(&job.out("result.mp4"), &result, &clip, expected, true);
        let size = format!("scale={}:{}", expected.0, expected.1);
        let (reference, produced) = (
            work.path().join(format!("ref{degrees}.png")),
            work.path().join(format!("out{degrees}.png")),
        );
        first_frame(&clip, &size, &reference);
        first_frame(&job.out("result.mp4"), "null", &produced);
        let score = ssim(&produced, &reference);
        record(
            "IT-10-9 orientation",
            &format!("rotation {degrees}: SSIM {score:.4} against the harness's upright render"),
        );
        assert!(score >= 0.75, "rotation {degrees}: {score}");
    }

    let vfr = clips.vfr_beeps();
    let (job, result, _) = run(&mut decoder, &mut peer, &vfr, plan(false, AudioPlan::Aac));
    assert_compliant(&job.out("result.mp4"), &result, &vfr, (640, 360), true);
    let (source, produced) = (beep_onsets(&vfr, 3), beep_onsets(&job.out("result.mp4"), 3));
    record(
        "IT-10-9 sync",
        &format!("beep onsets: source {source:?}, output {produced:?} (nominal 1.0, 3.0, 5.0 s)"),
    );
    assert_eq!(produced.len(), source.len(), "{source:?} vs {produced:?}");
    assert!(produced.len() >= 3, "{produced:?}");
    for (before, after) in source.iter().zip(&produced) {
        assert!(
            (before - after).abs() <= 0.040,
            "{source:?} vs {produced:?}"
        );
    }
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_10_hlg_becomes_bt709_sdr_within_the_luma_tolerance_of_the_harness_chain() {
    let clips = Clips::new();
    let hlg = clips.hlg(3840, 2160);
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let mut peer = decoder.connect();

    let (job, result, wall) = run(&mut decoder, &mut peer, &hlg, plan(true, AudioPlan::Aac));

    assert_compliant(&job.out("result.mp4"), &result, &hlg, (1920, 1080), true);
    let chain = format!("scale=1920:1080:flags=bicubic,{HDR_CHAIN}");
    let produced = mean_yavg(&job.out("result.mp4"), "null");
    let reference = mean_yavg(&hlg, &chain);
    let untouched = mean_yavg(&hlg, "scale=1920:1080:flags=bicubic");
    record(
        "IT-10-10",
        &format!(
            "synthetic 4K HLG clip (2 s, HEVC Main10): output 1920x1080 yuv420p bt709, mean YAVG {produced:.2}, harness chain {reference:.2}, no tone map {untouched:.2}; tolerance 2.0 (gate S5, synthetic), wall {wall:.1?}"
        ),
    );
    assert!(
        (produced - reference).abs() <= 2.0,
        "{produced} vs {reference}"
    );
    assert!(
        (produced - untouched).abs() > 5.0,
        "{produced} vs {untouched}"
    );
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_11_a_remux_copies_the_packets_and_moves_the_index_first() {
    let clips = Clips::new();
    let moov_last = clips.sdr("end.mp4", 1280, 720, 3, true);
    let variant = clips.tagged();
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let mut peer = decoder.connect();

    for (name, clip) in [
        ("moov at end", &moov_last),
        ("subtitle and chapters", &variant),
    ] {
        let (job, result, remux_wall) = run(&mut decoder, &mut peer, clip, Plan::Remux);
        let (_, _, transcode_wall) =
            run(&mut decoder, &mut peer, clip, plan(false, AudioPlan::Aac));

        assert_remux(clip, &job.out("result.mp4"), &result);
        record(
            "IT-10-11",
            &format!(
                "{name}: remux {remux_wall:.2?}, transcode of the same clip {transcode_wall:.2?} (AC-3 asks at most 20 %; asserted only as remux < transcode on a loaded host)"
            ),
        );
        assert!(remux_wall < transcode_wall, "{name}");
    }
    decoder.finish();
}

/// SIGSTOPs every ffmpeg in the container (the pid inside the container is not the host's).
fn stop_ffmpeg(decoder: &Decoder) {
    let script = "for p in /proc/[0-9]*; do read -r c < $p/comm; \
        if [ \"$c\" = ffmpeg ]; then kill -STOP ${p#/proc/}; fi; done";
    docker_ok(&["exec", &decoder.name, "sh", "-c", script]);
}

/// Milliseconds the guard waited for a progress block before it killed ffmpeg, from the runner's log.
fn silent_ms(logs: &str) -> u64 {
    let line = logs
        .lines()
        .find(|line| line.contains("Killed(Stall)"))
        .unwrap_or_else(|| panic!("no stall kill in the runner log:\n{logs}"));
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

fn slow_job(decoder: &mut Decoder, clips: &Clips) -> Job {
    decoder.job(&fs::read(clips.noisy(1920, 1080, 8)).expect("clip"))
}

fn one_cpu(job: &Job) -> Request {
    let mut limits = video_limits();
    limits.cpu = 1;
    Request::Transcode {
        job_dir: job.dir(),
        plan: plan(false, AudioPlan::None),
        limits,
    }
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_13_a_stopped_ffmpeg_is_killed_as_stalled_and_the_runner_restarts() {
    let clips = Clips::new();
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let before = decoder.state();
    let job = slow_job(&mut decoder, &clips);
    let mut peer = decoder.connect();
    peer.send(one_cpu(&job));
    peer.wait_for_progress(WAIT);

    stop_ffmpeg(&decoder);
    let answer = peer.response(WAIT);

    assert_eq!(answer, Response::Error(DecodeError::Stalled));
    let silent = silent_ms(&decoder.logs());
    record(
        "IT-10-13a",
        &format!(
            "SIGSTOP of ffmpeg mid-encode with decoder_stall_kill=3s: killed {silent} ms after its last progress block (3000 wanted)"
        ),
    );
    assert!((3000..13_000).contains(&silent), "{silent} ms");
    let mut again = decoder.wait_restarted(&before, Duration::from_secs(60));
    assert!(!job.work().exists(), "work/ is removed");
    assert!(
        only_the_runner_runs(&decoder.name),
        "{:?}",
        processes_of(&decoder.name)
    );
    assert!(matches!(
        again.call(Request::Hello { protocol: 2 }, Duration::from_secs(5)),
        Response::Capabilities(_)
    ));
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_19_cancel_and_socket_loss_stop_a_real_transcode_and_the_runner_keeps_serving() {
    let clips = Clips::new();
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let before = decoder.state();
    let cancelled = slow_job(&mut decoder, &clips);
    let lost = slow_job(&mut decoder, &clips);
    let control = decoder.job(&fs::read(clips.sdr("c.mp4", 640, 360, 1, true)).expect("clip"));
    let mut peer = decoder.connect();

    // (a) an explicit cancel mid-encode
    peer.send(one_cpu(&cancelled));
    peer.wait_for_progress(WAIT);
    let cancel_sent = Instant::now();
    peer.send(Request::Cancel);
    let answer = peer.response(Duration::from_secs(5));
    let took = cancel_sent.elapsed();
    assert_eq!(answer, Response::Cancelled);
    record(
        "IT-10-19a",
        &format!("Cancelled {took:?} after the cancel (2 s wanted)"),
    );
    assert!(!cancelled.work().exists(), "work/ is removed by the cancel");
    assert!(
        only_the_runner_runs(&decoder.name),
        "{:?}",
        processes_of(&decoder.name)
    );

    // (b) the connection closes while ffmpeg encodes
    peer.send(one_cpu(&lost));
    wait_for("the encode of the second job", WAIT, || {
        lost.work().exists()
    });
    drop(peer);
    wait_for("ffmpeg to be killed", Duration::from_secs(10), || {
        only_the_runner_runs(&decoder.name)
    });
    wait_for("work/ to be removed", Duration::from_secs(10), || {
        !lost.work().exists()
    });

    // (c) a new connection and a new job, with no restart
    let mut again = decoder.connect();
    let served = again.call(request(&control, plan(false, AudioPlan::Aac)), WAIT);
    assert!(matches!(served, Response::TranscodeResult(_)), "{served:?}");
    let after = decoder.state();
    assert_eq!(
        (after.started_at, after.restart_count),
        (before.started_at, before.restart_count)
    );
    decoder.finish();
}
