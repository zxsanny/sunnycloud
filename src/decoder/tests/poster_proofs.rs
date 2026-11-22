//! AZ-3859 through the real runner binary and the host's real ffmpeg and libvips: the offset of a
//! job is ignored for an image, only a PNG the runner made reaches vips for a video poster, and
//! the HLG poster is recorded (SDR tagged, SSIM against the harness chains, no pass criterion:
//! the tolerance is the owner's open item G-10-6).
mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::media::{
    Media, MediaRunner, image_result, limits, path_with_scripts, recorded_calls, recording_script,
    render, render_at, run_tool,
};
use common::reference::{record, reference_thumbnail, ssim, text};
use tempfile::TempDir;

fn result_bytes(job: &Path) -> Vec<u8> {
    fs::read(job.join("out/result.webp")).expect("out/result.webp")
}

#[test]
fn a_poster_offset_has_no_effect_on_an_image_the_output_bytes_equal_a_render_without_one() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.image("photo.jpg", 800, 600));
    image_result(render(&mut client, &job, 320, limits()));
    let without_offset = result_bytes(&job);

    let with_offset = image_result(render_at(&mut client, &job, 320, 5000));

    assert_eq!(result_bytes(&job), without_offset);
    assert_eq!((with_offset.width, with_offset.height), (320, 240));
    assert_eq!(with_offset.bytes, without_offset.len() as u64);
}

#[test]
fn vips_reads_only_the_png_the_runner_made_for_a_video_poster_and_never_the_job_input() {
    let media = Media::new();
    let logs = TempDir::new().expect("log dir");
    let (poster_log, image_log) = (
        logs.path().join("poster.log"),
        logs.path().join("image.log"),
    );
    let run = |log: &Path, bytes: &[u8]| {
        let vips = recording_script("vips", log);
        let vipsheader = recording_script("vipsheader", log);
        let (_bin, path) = path_with_scripts(&[("vips", &vips), ("vipsheader", &vipsheader)]);
        let mut runner = MediaRunner::start_with(&[], &path);
        let mut client = runner.connect();
        let job = runner.job(bytes);
        let result = render_at(&mut client, &job, 160, 300);
        (image_result(result), job)
    };

    let (_poster, video_job) = run(&poster_log, &media.mp4(3));
    let (_still, image_job) = run(&image_log, &media.image("photo.jpg", 800, 600));

    let video_calls = recorded_calls(&poster_log);
    assert!(!video_calls.is_empty());
    let source = video_job.join("in/source");
    for call in &video_calls {
        let input = call.first().expect("the input line");
        assert!(
            input.starts_with("input ")
                && (input.ends_with("/work/poster.png") || input.ends_with("/out/result.webp")),
            "vips was given {input}: neither the poster frame nor the runner's own output"
        );
        assert!(
            call.iter()
                .filter(|line| line.starts_with("arg "))
                .all(|line| !line.contains(text(&video_job))),
            "a job path is in the argv: {call:?}"
        );
        assert!(!input.contains(text(&source)));
    }
    // The control: for an image the same recording shows the job's own input, so it can tell.
    let image_calls = recorded_calls(&image_log);
    assert!(!image_calls.is_empty());
    assert!(
        image_calls
            .iter()
            .any(|call| call[0].ends_with(text(&image_job.join("in/source")))),
        "{image_calls:?}"
    );
}

/// The decoder's tone-mapping chain (10 section 13.2), written out again here so that the
/// reference does not come from the runner's code.
const HDR_CHAIN: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p,format=rgb24";

/// The frame at `at` seconds of `clip` as a PNG, through `filter`.
fn frame_through(clip: &Path, at: &str, filter: &str, out: &Path) {
    run_tool(
        "ffmpeg",
        &[
            "-loglevel",
            "error",
            "-y",
            "-ss",
            at,
            "-i",
            text(clip),
            "-frames:v",
            "1",
            "-vf",
            filter,
            text(out),
        ],
    );
}

#[test]
fn an_hlg_poster_is_sdr_and_its_ssim_against_the_harness_chains_is_recorded_without_a_criterion() {
    let media = Media::new();
    let bytes = media.hlg_mp4();
    let clip = media.path("hlg.mp4");
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&bytes);
    let work = TempDir::new().expect("harness work directory");

    let result = image_result(render_at(&mut client, &job, 320, 500));

    let poster = job.join("out/result.webp");
    let interpretation = Command::new("vipsheader")
        .args(["-f", "interpretation"])
        .arg(&poster)
        .output()
        .expect("vipsheader");
    let tagged = String::from_utf8_lossy(&interpretation.stdout).to_lowercase();
    assert!(tagged.contains("srgb"), "{tagged}");
    assert_eq!((result.width, result.height), (320, 240));
    let tone_mapped = work.path().join("chain.png");
    let plain = work.path().join("plain.png");
    frame_through(&clip, "0.5", HDR_CHAIN, &tone_mapped);
    frame_through(&clip, "0.5", "scale=iw:ih", &plain);
    let (chain_reference, plain_reference) = (
        work.path().join("chain.webp"),
        work.path().join("plain.webp"),
    );
    reference_thumbnail(&tone_mapped, 320, &chain_reference);
    reference_thumbnail(&plain, 320, &plain_reference);
    record(
        "AZ-3859 AC-4 host",
        &format!(
            "HLG poster (synthetic 320x240 HEVC Main10 HLG clip): SDR tagged (srgb); SSIM {:.4} against the harness tone-map chain, {:.4} against no tone map; recorded, no criterion (G-10-6)",
            ssim(&poster, &chain_reference),
            ssim(&poster, &plain_reference),
        ),
    );
}
