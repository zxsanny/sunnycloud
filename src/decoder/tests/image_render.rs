//! AZ-3857 through the real runner binary, the real protocol and the host's real libvips:
//! the sniff, the header read, the two WebP sizes, the orientation and the typed failures.
mod common;

use std::path::Path;

use common::media::{
    Media, MediaRunner, WAIT, header_of, image_result, limits, png_header_only, probe, render,
};
use media_shared::decoder_protocol::{DecodeError, ImageResult, LimitKind, Response};
use media_shared::policy::MediaKind;

fn result_file(job: &Path) -> std::path::PathBuf {
    job.join("out/result.webp")
}

/// The reply is real: the file at `out/result.webp` is a WebP that says the same.
fn assert_reply_matches_file(job: &Path, result: ImageResult) {
    let file = std::fs::read(result_file(job)).expect("result file");
    assert_eq!(&file[..4], b"RIFF");
    assert_eq!(&file[8..12], b"WEBP");
    assert_eq!(result.bytes, file.len() as u64);
    assert_eq!(header_of(&result_file(job)), (result.width, result.height));
}

#[test]
fn each_supported_format_renders_a_thumbnail_and_a_preview_of_the_requested_edge() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let inputs = [
        ("jpg", media.image("photo.jpg", 2000, 1500)),
        ("png", media.image("photo.png", 2000, 1500)),
        ("webp", media.image("photo.webp", 2000, 1500)),
        ("gif", media.image("photo.gif", 2000, 1500)),
        ("heic", media.image("photo.heic", 2000, 1500)),
    ];

    for (format, bytes) in inputs {
        let job = runner.job(&bytes);
        let thumb = image_result(render(&mut client, &job, 320, limits()));
        assert_reply_matches_file(&job, thumb);
        let preview = image_result(render(&mut client, &job, 1440, limits()));
        assert_reply_matches_file(&job, preview);

        assert_eq!((thumb.width, thumb.height), (320, 240), "{format} thumb");
        assert_eq!(
            (preview.width, preview.height),
            (1440, 1080),
            "{format} preview"
        );
    }
}

#[test]
fn exif_orientation_six_swaps_the_dimensions_and_the_output_carries_no_exif() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.oriented_jpeg(2000, 1500, 6));

    let result = image_result(render(&mut client, &job, 320, limits()));

    assert_eq!((result.width, result.height), (240, 320));
    let file = std::fs::read(result_file(&job)).expect("result");
    assert!(
        !file
            .windows(4)
            .any(|w| w == b"EXIF" || w == b"ICCP" || w == b"XMP ")
    );
}

#[test]
fn a_heic_with_a_rotation_transform_comes_out_upright() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.oriented_heic(2000, 1500, 6));

    let result = image_result(render(&mut client, &job, 320, limits()));

    assert_eq!((result.width, result.height), (240, 320));
    assert_reply_matches_file(&job, result);
}

#[test]
fn a_small_image_is_not_enlarged_to_the_requested_edge() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.image("small.png", 200, 100));

    let result = image_result(render(&mut client, &job, 1440, limits()));

    assert_eq!((result.width, result.height), (200, 100));
}

#[test]
fn a_stale_result_file_is_replaced_and_work_is_gone_after_the_job() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.image("photo.jpg", 800, 600));
    std::fs::write(result_file(&job), b"stale").expect("stale file");

    let result = image_result(render(&mut client, &job, 320, limits()));

    assert_reply_matches_file(&job, result);
    assert!(!job.join("work").exists());
}

#[test]
fn a_truncated_jpeg_header_is_corrupt_and_a_zero_byte_file_is_unsupported() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let cut_in_header = runner.job(&media.image("photo.jpg", 800, 600)[..100]);
    let empty = runner.job(&[]);

    let corrupt = render(&mut client, &cut_in_header, 320, limits());
    let unsupported = render(&mut client, &empty, 320, limits());

    assert_eq!(corrupt, Response::Error(DecodeError::Corrupt));
    assert_eq!(unsupported, Response::Error(DecodeError::Unsupported));
    assert!(!result_file(&cut_in_header).exists());
}

#[test]
fn a_jpeg_cut_inside_its_pixel_data_renders_the_part_libvips_can_read() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let bytes = media.image("photo.jpg", 800, 600);
    let job = runner.job(&bytes[..bytes.len() / 2]);

    let response = render(&mut client, &job, 320, limits());

    // libvips keeps the decoded rows and reports a warning, exit code 0: a recorded outcome,
    // not a promise; the answer must be typed either way and never a hang.
    match response {
        Response::ImageResult(result) => assert_reply_matches_file(&job, result),
        Response::Error(DecodeError::Corrupt) => {}
        other => panic!("unexpected answer {other:?}"),
    }
}

#[test]
fn files_that_are_not_images_are_unsupported_whatever_they_are_named() {
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let svg = runner.job(b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"9\" height=\"9\"/>");
    let pdf = runner.job(b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n");
    let playlist = runner.job(b"#EXTM3U\n#EXTINF:1,\nfile:///etc/passwd\n");

    for (name, job) in [("svg", svg), ("pdf", pdf), ("playlist", playlist)] {
        let response = render(&mut client, &job, 320, limits());

        assert_eq!(
            response,
            Response::Error(DecodeError::Unsupported),
            "{name}"
        );
        assert!(!result_file(&job).exists(), "{name}");
    }
}

#[test]
fn an_image_above_the_pixel_limit_is_refused_from_its_header_and_one_at_the_limit_renders() {
    let media = Media::new();
    let mut runner = MediaRunner::start_with(
        &[("SUNNYCLOUD_PHOTO_MAX_PIXELS", "1000000")],
        &std::env::var("PATH").expect("PATH"),
    );
    let mut client = runner.connect();
    let at_limit = runner.job(&media.image("exact.png", 1000, 1000));
    let above = runner.job(&media.image("above.png", 1001, 1000));

    let rendered = render(&mut client, &at_limit, 320, limits());
    let refused = render(&mut client, &above, 320, limits());

    assert!(matches!(rendered, Response::ImageResult(_)), "{rendered:?}");
    assert_eq!(
        refused,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Pixels
        })
    );
    assert!(!result_file(&above).exists());
}

#[test]
fn a_decompression_bomb_header_of_900_megapixels_is_refused_at_once_with_the_default_limit() {
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&png_header_only(30_000, 30_000));

    let started = std::time::Instant::now();
    let response = render(&mut client, &job, 320, limits());

    assert_eq!(
        response,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Pixels
        })
    );
    assert!(started.elapsed() < WAIT / 4, "{:?}", started.elapsed());
}

#[test]
fn an_output_above_the_size_limit_is_never_reported_as_a_result() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&media.image("photo.jpg", 2000, 1500));
    let mut small = limits();
    small.max_output_bytes = 2048;

    let response = render(&mut client, &job, 1440, small);

    assert_eq!(
        response,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Output
        })
    );
}

#[test]
fn a_photo_probe_reads_the_stored_dimensions_from_the_header() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let cases = [
        ("jpeg", media.image("p.jpg", 640, 480)),
        ("png", media.image("p.png", 640, 480)),
        ("webp", media.image("p.webp", 640, 480)),
        ("gif", media.image("p.gif", 640, 480)),
        ("heif", media.image("p.heic", 640, 480)),
    ];

    for (container, bytes) in cases {
        let job = runner.job(&bytes);

        let Response::Probe(facts) = probe(&mut client, &job) else {
            panic!("{container}: expected a probe");
        };

        assert_eq!(facts.kind, MediaKind::Photo, "{container}");
        assert_eq!(facts.container, container);
        let video = facts.video.expect("photo dimensions");
        assert_eq!((video.width, video.height), (640, 480), "{container}");
    }
}

#[test]
fn a_photo_probe_of_a_900_megapixel_png_header_answers_with_its_dimensions_without_decoding() {
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let job = runner.job(&png_header_only(30_000, 30_000));

    let started = std::time::Instant::now();
    let response = probe(&mut client, &job);

    let Response::Probe(facts) = response else {
        panic!("expected a probe, got {response:?}");
    };
    let video = facts.video.expect("dimensions");
    assert_eq!((video.width, video.height), (30_000, 30_000));
    assert!(started.elapsed() < WAIT / 4, "{:?}", started.elapsed());
}
