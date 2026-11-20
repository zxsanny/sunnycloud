//! AZ-3858 and AZ-3859 through the real runner binary and the host's real ffmpeg and ffprobe:
//! the video probe, the box scan and the poster frame at a requested time.
mod common;

use std::path::Path;

use common::media::{
    Media, MediaRunner, ffprobe_fields, header_of, image_result, mean_of, probe, render_at,
};
use media_shared::decoder_protocol::{DecodeError, Response};
use media_shared::policy::{MediaKind, Probe};

fn probed(runner: &mut MediaRunner, bytes: &[u8]) -> Probe {
    let mut client = runner.connect();
    let job = runner.job(bytes);
    match probe(&mut client, &job) {
        Response::Probe(facts) => facts,
        other => panic!("expected a probe, got {other:?}"),
    }
}

fn result_file(job: &Path) -> std::path::PathBuf {
    job.join("out/result.webp")
}

#[test]
fn a_video_probe_reports_the_facts_an_independent_ffprobe_reports() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let bytes = media.mp4(3);

    let facts = probed(&mut runner, &bytes);

    let video = facts.video.expect("video stream");
    let independent = ffprobe_fields(
        &media.path("clip.mp4"),
        "stream=codec_name,profile,width,height,pix_fmt",
    );
    assert_eq!(facts.kind, MediaKind::Video);
    assert!(
        facts.container.split(',').any(|name| name == "mp4"),
        "{}",
        facts.container
    );
    assert_eq!(
        format!(
            "{},{},{},{},{}",
            video.codec.expect("codec"),
            video.profile.expect("profile"),
            video.width,
            video.height,
            video.pix_fmt.expect("pix_fmt")
        ),
        independent
    );
    let duration = facts.duration_ms.expect("duration");
    assert!((2900..=3100).contains(&duration), "{duration}");
    assert_eq!(facts.audio, None);
}

#[test]
fn moov_first_follows_the_position_of_the_index() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let at_end = media.mp4(2);
    let first = media.faststart_mp4(2);

    let end_facts = probed(&mut runner, &at_end);
    let first_facts = probed(&mut runner, &first);

    assert!(!end_facts.moov_first);
    assert!(first_facts.moov_first);
}

#[test]
fn a_rotated_clip_reports_its_display_matrix_angle_and_a_quicktime_file_is_probed() {
    let media = Media::new();
    let mut runner = MediaRunner::start();

    let rotated = probed(&mut runner, &media.rotated_mp4());
    let mov = probed(&mut runner, &media.mov());

    assert_eq!(rotated.video.expect("video").rotation, Some(90));
    assert_eq!(mov.kind, MediaKind::Video);
    assert!(mov.container.contains("mov"));
}

#[test]
fn an_hlg_clip_reports_its_transfer_characteristic() {
    let media = Media::new();
    let mut runner = MediaRunner::start();

    let facts = probed(&mut runner, &media.hlg_mp4());

    let video = facts.video.expect("video");
    assert_eq!(video.transfer.as_deref(), Some("arib-std-b67"));
    assert_eq!(video.codec.as_deref(), Some("hevc"));
}

#[test]
fn a_damaged_clip_is_a_typed_failure_for_the_probe_and_the_poster() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let clip = media.mp4(2);
    let job = runner.job(&clip[..2000]);

    let probe_answer = probe(&mut client, &job);
    let poster_answer = render_at(&mut client, &job, 320, 0);

    assert_eq!(probe_answer, Response::Error(DecodeError::Corrupt));
    assert_eq!(poster_answer, Response::Error(DecodeError::Corrupt));
    assert!(!result_file(&job).exists());
}

#[test]
fn the_poster_is_the_frame_nearest_the_requested_time() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.stepped_clip());

    let at_1000 = image_result(render_at(&mut client, &job, 320, 1000));
    let bright = mean_of(&result_file(&job));
    let at_900 = image_result(render_at(&mut client, &job, 320, 900));
    let darker = mean_of(&result_file(&job));
    let at_2000 = image_result(render_at(&mut client, &job, 320, 2000));
    let brightest = mean_of(&result_file(&job));

    // Frame n is a flat grey of luma 8n: 10 frames per second, so 900 ms is one frame before
    // 1000 ms (a step of about 9 in the RGB mean) and 2000 ms is twenty frames in.
    assert!((bright - 74.0).abs() < 3.0, "{bright}");
    assert!((darker - 65.0).abs() < 3.0, "{darker}");
    assert!(brightest > 150.0, "{brightest}");
    for result in [at_1000, at_900, at_2000] {
        assert_eq!((result.width, result.height), (64, 64));
    }
}

#[test]
fn an_mp4_named_as_a_jpeg_renders_a_poster_of_the_requested_edge() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.mp4(3));

    let result = image_result(render_at(&mut client, &job, 160, 300));

    assert_eq!((result.width, result.height), (160, 120));
    assert_eq!(header_of(&result_file(&job)), (160, 120));
    assert!(!job.join("work").exists());
}

#[test]
fn a_rotated_clip_gets_an_upright_poster() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.rotated_mp4());

    let result = image_result(render_at(&mut client, &job, 320, 100));

    assert_eq!((result.width, result.height), (240, 320));
}

#[test]
fn an_hlg_clip_gets_a_tone_mapped_poster() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.hlg_mp4());

    let result = image_result(render_at(&mut client, &job, 320, 500));

    assert_eq!((result.width, result.height), (320, 240));
    assert!(mean_of(&result_file(&job)) > 10.0);
}

#[test]
fn a_time_beyond_the_end_of_the_clip_is_an_invalid_output_not_a_hang() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.mp4(2));

    let response = render_at(&mut client, &job, 320, 60_000);

    assert_eq!(response, Response::Error(DecodeError::OutputInvalid));
}

#[test]
fn a_job_offset_is_ignored_for_an_image() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.image("photo.jpg", 800, 600));

    let with_offset = image_result(render_at(&mut client, &job, 320, 5000));

    assert_eq!((with_offset.width, with_offset.height), (320, 240));
}
