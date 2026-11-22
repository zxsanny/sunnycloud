//! AZ-3860 through the real runner binary and the host's real ffmpeg and ffprobe on generated clips:
//! what a transcode and a remux produce (codec, size caps, tags, streams, index, duration, orientation,
//! HDR, bitrate). Every reference is made by the host's own tools; no encoder bytes are compared.
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use common::media::{MediaRunner, cpu_plan, transcode, video_limits};
use common::reference::{record, ssim, text};
use common::video_assert::{assert_compliant, assert_remux};
use common::video_measure::{beep_onsets, bitrate_windows, first_frame, mean_luma, psnr};
use common::video_ref::{Clips, HDR_CHAIN, report_of, streams_of, tag_keys};
use media_shared::decoder_protocol::{Event, Response, TranscodeResult};
use media_shared::policy::{AudioPlan, Plan, is_compliant};
use tempfile::TempDir;

struct Done {
    result: TranscodeResult,
    output: PathBuf,
    events: Vec<Event>,
    wall: std::time::Duration,
}

fn run(runner: &mut MediaRunner, clip: &Path, plan: Plan) -> Done {
    let mut client = runner.connect();
    let job = runner.job(&fs::read(clip).expect("clip"));
    let started = Instant::now();
    let (response, events) = transcode(&mut client, &job, plan, video_limits());
    let wall = started.elapsed();
    let Response::TranscodeResult(result) = response else {
        panic!(
            "{}: expected a result, got {response:?}; runner log:\n{}",
            clip.display(),
            runner.log()
        )
    };
    Done {
        result,
        output: job.join("out/result.mp4"),
        events,
        wall,
    }
}

fn assert_compliant_done(done: &Done, input: &Path, expected: (u64, u64), audio: bool) {
    assert_compliant(&done.output, &done.result, input, expected, audio);
}

#[test]
fn a_landscape_sdr_clip_becomes_a_compliant_1080p_h264_aac_mp4() {
    let clips = Clips::new();
    let clip = clips.sdr("big.mp4", 2560, 1440, 2, true);
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

    assert_compliant_done(&done, &clip, (1920, 1080), true);
    let report = report_of(&done.output);
    let rate: u64 = streams_of(&report, "audio")[0]["bit_rate"]
        .as_str()
        .expect("bit_rate")
        .parse()
        .expect("rate");
    assert!(
        (90_000..=150_000).contains(&rate),
        "a steady tone is coded at about 128 kbit/s, not {rate}"
    );
}

#[test]
fn a_clip_already_at_1080p_keeps_its_size() {
    let clips = Clips::new();
    let clip = clips.sdr("hd.mp4", 1920, 1080, 1, true);
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

    assert_compliant_done(&done, &clip, (1920, 1080), true);
}

#[test]
fn a_portrait_clip_is_bounded_by_the_same_caps_with_the_sides_swapped() {
    let clips = Clips::new();
    let clip = clips.sdr("tall.mp4", 1440, 2560, 2, true);
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

    assert_compliant_done(&done, &clip, (1080, 1920), true);
}

#[test]
fn a_clip_without_audio_gets_no_audio_stream_whether_the_plan_says_none_or_aac() {
    let clips = Clips::new();
    let clip = clips.sdr("silent.mp4", 640, 360, 2, false);
    let mut runner = MediaRunner::start();

    let none = run(&mut runner, &clip, cpu_plan(false, AudioPlan::None));
    let aac = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

    assert_compliant_done(&none, &clip, (640, 360), false);
    assert_compliant_done(&aac, &clip, (640, 360), false);
}

#[test]
fn the_audio_plan_none_drops_the_audio_of_a_clip_that_has_some() {
    let clips = Clips::new();
    let clip = clips.sdr("loud.mp4", 640, 360, 2, true);
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::None));

    assert_compliant_done(&done, &clip, (640, 360), false);
}

#[test]
fn odd_sides_come_out_even() {
    let clips = Clips::new();
    let clip = clips.odd_sided();
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::None));

    assert_compliant_done(&done, &clip, (320, 240), false);
}

#[test]
fn a_rotated_clip_comes_out_upright_with_no_rotation_tag() {
    let clips = Clips::new();
    let mut runner = MediaRunner::start();
    let work = TempDir::new().expect("work dir");
    for degrees in [90, 180, 270] {
        let clip = clips.rotated(&format!("rot{degrees}.mp4"), 640, 360, degrees);
        let expected = if degrees == 180 {
            (640, 360)
        } else {
            (360, 640)
        };

        let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

        assert_compliant_done(&done, &clip, expected, true);
        // The reference is the same installed ffmpeg's own upright render of the input; a
        // missing or doubled rotation is a different picture. The control is the stored
        // orientation: it must score lower, or the check would not tell the cases apart.
        let size = format!("scale={}:{}", expected.0, expected.1);
        let (reference, output, stored) = (
            work.path().join(format!("ref{degrees}.png")),
            work.path().join(format!("out{degrees}.png")),
            work.path().join(format!("stored{degrees}.png")),
        );
        first_frame(&clip, &size, &reference);
        first_frame(&done.output, "null", &output);
        let noautorotate = format!("{size},null");
        run_noautorotate(&clip, &noautorotate, &stored);
        let (score, control) = (ssim(&output, &reference), ssim(&stored, &reference));
        record(
            "AZ-3860 orientation",
            &format!(
                "rotation {degrees}: SSIM {score:.4} against the upright reference, {control:.4} for the stored orientation"
            ),
        );
        assert!(score >= 0.75, "rotation {degrees}: SSIM {score}");
        assert!(
            score - control >= 0.2,
            "rotation {degrees}: the control scores {control}"
        );
    }
}

fn run_noautorotate(clip: &Path, filter: &str, out: &Path) {
    common::media::run_tool(
        "ffmpeg",
        &[
            "-loglevel",
            "error",
            "-y",
            "-noautorotate",
            "-i",
            text(clip),
            "-vf",
            filter,
            "-frames:v",
            "1",
            text(out),
        ],
    );
}

#[test]
fn an_hlg_clip_becomes_bt709_sdr_with_the_luma_of_the_harness_chain() {
    let clips = Clips::new();
    let clip = clips.hlg(1280, 720);
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(true, AudioPlan::Aac));

    assert_compliant_done(&done, &clip, (1280, 720), true);
    let chain = format!("scale=1280:720:flags=bicubic,{HDR_CHAIN}");
    let produced = mean_luma(&done.output, "null");
    let reference = mean_luma(&clip, &chain);
    let untouched = mean_luma(&clip, "scale=1280:720");
    record(
        "AZ-3860 AC-4 host",
        &format!(
            "synthetic 1280x720 HLG clip: mean luma {produced:.2}, harness chain {reference:.2}, no tone map {untouched:.2} (tolerance 2.0 proposed by gate S5)"
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
    assert_eq!(
        done.result
            .probe_of_output
            .video
            .as_ref()
            .and_then(|v| v.transfer.as_deref()),
        Some("bt709")
    );
}

#[test]
fn metadata_subtitles_chapters_and_extra_tags_are_not_carried_over() {
    let clips = Clips::new();
    let clip = clips.tagged();
    let before = report_of(&clip);
    assert_eq!(
        streams_of(&before, "subtitle").len(),
        1,
        "the source has a subtitle"
    );
    assert!(
        tag_keys(&before)
            .iter()
            .any(|key| key == "location" || key == "title")
    );
    assert!(!before["chapters"].as_array().expect("chapters").is_empty());
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

    assert_compliant_done(&done, &clip, (640, 360), true);
    let after = report_of(&done.output);
    let forbidden = ["title", "location", "com.apple.quicktime.location.iso6709"];
    let carried: Vec<String> = tag_keys(&after)
        .into_iter()
        .filter(|key| forbidden.contains(&key.as_str()))
        .collect();
    assert!(carried.is_empty(), "tags carried over: {carried:?}");
    assert!(after["chapters"].as_array().expect("chapters").is_empty());
    let dump = fs::read(&done.output).expect("output bytes");
    for secret in ["Secret title", "Secret stream title", "+50.4501"] {
        assert!(
            !dump.windows(secret.len()).any(|w| w == secret.as_bytes()),
            "{secret}"
        );
    }
}

/// A saturating clip (its source is far above any cap) comes out at most 10 Mbit/s as ffprobe
/// reports it, and passes the supervisor's own compliance rule.
fn assert_saturating_clip_is_held(name: &str, width: u32, height: u32, seconds: u32) {
    let clips = Clips::new();
    let clip = clips.noisy(width, height, seconds);
    let (_, source_rate) = bitrate_windows(&clip);
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::None));

    let (worst, average) = bitrate_windows(&done.output);
    let report = report_of(&done.output);
    let probed: u64 = streams_of(&report, "video")[0]["bit_rate"]
        .as_str()
        .expect("ffprobe reports a video bit_rate")
        .parse()
        .expect("bit_rate");
    record(
        "AZ-3860 maxrate",
        &format!(
            "{name}: source {:.1} Mbit/s, ffprobe video bit_rate {:.2} Mbit/s, packet average {:.2} Mbit/s, worst 1 s window {:.2} Mbit (-maxrate 8M -bufsize 4M)",
            source_rate / 1e6,
            probed as f64 / 1e6,
            average / 1e6,
            worst as f64 / 1e6
        ),
    );
    assert!(
        source_rate > 20e6,
        "the source must exceed the cap to prove it: {source_rate}"
    );
    assert!(probed <= 10_000_000, "{name}: ffprobe bit_rate {probed}");
    assert_eq!(
        done.result
            .probe_of_output
            .video
            .as_ref()
            .and_then(|v| v.bitrate),
        Some(probed),
        "the reported probe carries the same bit rate"
    );
    assert!(average <= 10e6, "{name}: packet average {average}");
    assert!(
        is_compliant(&done.result.probe_of_output),
        "{name}: the output fails the supervisor's compliance rule: {:?}",
        done.result.probe_of_output
    );
    // The VBV bound: a window carries at most maxrate plus the buffer.
    assert!(worst <= 12_000_000, "{name}: worst window {worst}");
}

#[test]
fn a_short_saturating_clip_comes_out_at_most_10_mbit_and_compliant() {
    assert_saturating_clip_is_held("4 s noisy 1080p", 1920, 1080, 4);
}

#[test]
fn a_long_saturating_clip_comes_out_at_most_10_mbit_and_compliant() {
    assert_saturating_clip_is_held("20 s noisy 720p", 1280, 720, 20);
}

#[test]
fn a_transcode_sends_progress_events_that_stay_within_the_clip() {
    let clips = Clips::new();
    let clip = clips.noisy(1920, 1080, 4);
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::None));

    let times: Vec<u64> = done
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Progress { out_time_ms, .. } => Some(*out_time_ms),
            Event::Alive => None,
        })
        .collect();
    assert!(!times.is_empty(), "no progress event in {:?}", done.events);
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]), "{times:?}");
    assert!(times.iter().all(|&ms| ms <= 4_500), "{times:?}");
}

#[test]
fn a_remux_copies_the_packets_and_moves_the_index_to_the_front() {
    let clips = Clips::new();
    let clip = clips.tagged();
    let mut runner = MediaRunner::start();
    let transcoded = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

    let done = run(&mut runner, &clip, Plan::Remux);

    assert_remux(&clip, &done.output, &done.result);
    record(
        "AZ-3860 remux wall",
        &format!(
            "remux {:?}, transcode of the same clip {:?} (AC-3 asks at most 20 %; only meaningful on a quiet host)",
            done.wall, transcoded.wall
        ),
    );
}

#[test]
fn a_variable_rate_clip_keeps_its_beeps_within_40_ms_of_where_the_source_has_them() {
    let clips = Clips::new();
    let clip = clips.vfr_beeps();
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::Aac));

    assert_compliant_done(&done, &clip, (640, 360), true);
    let (source, produced) = (beep_onsets(&clip, 3), beep_onsets(&done.output, 3));
    record(
        "AZ-3860 sync",
        &format!("beep onsets: source {source:?}, output {produced:?} (nominal 1.0, 3.0, 5.0 s)"),
    );
    assert_eq!(source.len(), 3, "the source has three beeps: {source:?}");
    assert_eq!(produced.len(), 3, "{produced:?}");
    for (before, after) in source.iter().zip(&produced) {
        assert!(
            (before - after).abs() <= 0.040,
            "{source:?} vs {produced:?}"
        );
    }
}

const TO_BT709: &str = "zscale=p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709";
const RETAG_ONLY: &str = "setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709";

#[test]
fn a_bt601_clip_is_converted_to_bt709_like_the_harness_zscale_and_not_only_retagged() {
    let clips = Clips::new();
    let clip = clips.sdr_in("sd.mp4", false);
    let work = TempDir::new().expect("work dir");
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::None));

    assert_compliant_done(&done, &clip, (640, 360), false);
    let tags = done.result.probe_of_output.video.as_ref().expect("video");
    assert_eq!(
        (
            tags.primaries.as_deref(),
            tags.transfer.as_deref(),
            tags.matrix.as_deref()
        ),
        (Some("bt709"), Some("bt709"), Some("bt709"))
    );
    let (produced, converted, retagged) = (
        work.path().join("out.png"),
        work.path().join("converted.png"),
        work.path().join("retagged.png"),
    );
    first_frame(&done.output, "null", &produced);
    first_frame(&clip, TO_BT709, &converted);
    first_frame(&clip, RETAG_ONLY, &retagged);
    let (to_reference, to_retag) = (psnr(&produced, &converted), psnr(&produced, &retagged));
    let (ssim_reference, ssim_retag) = (ssim(&produced, &converted), ssim(&produced, &retagged));
    record(
        "AZ-3860 bt601",
        &format!(
            "BT.601 clip: output vs the harness zscale conversion PSNR {to_reference:.1} dB, SSIM {ssim_reference:.4}; vs a retag-only render PSNR {to_retag:.1} dB, SSIM {ssim_retag:.4}"
        ),
    );
    assert!(ssim_reference >= 0.98, "SSIM {ssim_reference}");
    assert!(
        to_reference >= to_retag + 3.0,
        "{to_reference} dB vs {to_retag} dB"
    );
}

#[test]
fn a_bt709_clip_is_not_changed_by_the_filter_chain() {
    let clips = Clips::new();
    let clip = clips.sdr_in("hd709.mp4", true);
    let work = TempDir::new().expect("work dir");
    let mut runner = MediaRunner::start();

    let done = run(&mut runner, &clip, cpu_plan(false, AudioPlan::None));

    assert_compliant_done(&done, &clip, (640, 360), false);
    let (produced, source) = (work.path().join("out.png"), work.path().join("in.png"));
    first_frame(&done.output, "null", &produced);
    first_frame(&clip, "null", &source);
    let (score, noise) = (ssim(&produced, &source), psnr(&produced, &source));
    record(
        "AZ-3860 bt709",
        &format!(
            "BT.709 clip: output vs source frame SSIM {score:.4}, PSNR {noise:.1} dB (re-encode only)"
        ),
    );
    // Only the lossy encode separates them: a colour shift would put the PSNR near 18 dB.
    assert!(score >= 0.97, "SSIM {score}");
    assert!(noise >= 38.0, "PSNR {noise}");
}
