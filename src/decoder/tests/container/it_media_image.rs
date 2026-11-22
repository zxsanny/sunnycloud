//! IT-10-4 to IT-10-6 on the release image (real libvips 8.18.7, libheif): probe is a header read,
//! `render_still` applies orientation and strips metadata, the pixel limit is checked before any decode.
//! Reference dimensions and pictures come from the harness's own host libvips and ffmpeg.

use std::fs;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{DecodeError, LimitKind, Response};
use media_shared::policy::{MediaKind, Probe};

use crate::common::media::{
    header_of, png_header_only, rotated_heic, run_tool, set_orientation_at,
};
use crate::decoder::{Decoder, Flavor, release_image};
use crate::host::{RssWatch, peak_rss};
use crate::peer::{photo_limits, probe, render};
use crate::refs::{
    pattern_heic, pattern_jpeg, pattern_png, record, reference_thumbnail, ssim, text, webp_chunks,
    workdir,
};

const WAIT: Duration = Duration::from_secs(30);
const MIB: u64 = 1 << 20;

fn dimensions(probed: &Probe) -> (u32, u32) {
    let video = probed
        .video
        .as_ref()
        .expect("a photo probe fills width and height");
    (video.width, video.height)
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_4_probe_answers_photo_dimensions_from_the_header() {
    let work = workdir();
    let jpeg = pattern_jpeg(work.path(), "p.jpg", 4000, 3000);
    pattern_heic(work.path(), "p.heic", 4000, 3000);
    let png = pattern_png(work.path(), "p.png", 4000, 3000);
    run_tool(
        "vips",
        &["webpsave", text(&png), text(&work.path().join("p.webp"))],
    );
    let gif = work.path().join("p.gif");
    run_tool(
        "ffmpeg",
        &[
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x480",
            "-frames:v",
            "1",
            text(&gif),
        ],
    );
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let mut peer = decoder.connect();
    let pid = decoder.runner_pid();
    let baseline = peak_rss(pid).expect("runner rss");

    for (name, file, twelve_mp) in [
        ("jpeg 12 MP", "p.jpg", true),
        ("heic 12 MP", "p.heic", true),
        ("png 12 MP", "p.png", true),
        ("webp 12 MP", "p.webp", true),
        ("gif 640x480", "p.gif", false),
    ] {
        let path = work.path().join(file);
        let (width, height) = header_of(&path);
        let job = decoder.job(&fs::read(&path).expect("generated file"));
        let mut times = Vec::new();
        let mut last = None;
        for _ in 0..5 {
            let started = Instant::now();
            last = Some(peer.call(probe(&job), WAIT));
            times.push(started.elapsed());
        }
        times.sort();
        let Some(Response::Probe(answer)) = last else {
            panic!("{name}: no probe answer")
        };
        assert_eq!(answer.kind, MediaKind::Photo, "{name}");
        assert_eq!(
            dimensions(&answer),
            (width, height),
            "{name}: equals the manifest"
        );
        let budget = if twelve_mp {
            "budget 50 ms"
        } else {
            "no budget"
        };
        record(
            "IT-10-4",
            &format!(
                "{name}: {width}x{height}, probe median {:?}, best {:?} ({budget})",
                times[2], times[0]
            ),
        );
    }
    let huge = decoder.job(&png_header_only(30_000, 30_000));
    let started = Instant::now();
    let answer = peer.call(probe(&huge), WAIT);
    let took = started.elapsed();
    let Response::Probe(huge_probe) = answer else {
        panic!("huge png: {answer:?}")
    };
    assert_eq!(dimensions(&huge_probe), (30_000, 30_000));
    record(
        "IT-10-4",
        &format!("png 30000x30000 header only: best of 5 probes {took:?} (budget 100 ms)"),
    );
    let truncated = decoder.job(&jpeg[..100]);
    let answer = peer.call(probe(&truncated), WAIT);
    assert!(
        matches!(
            answer,
            Response::Error(DecodeError::Corrupt) | Response::Probe(_)
        ),
        "truncated jpeg: {answer:?}"
    );
    record(
        "IT-10-4",
        &format!("truncated jpeg (100 bytes): {answer:?}"),
    );
    let growth = peak_rss(pid).expect("runner rss").saturating_sub(baseline);
    assert!(
        growth < 50 * MIB,
        "the probes grew the runner by {} MiB",
        growth / MIB
    );
    decoder.finish();
}

fn expected(orientation: u8, edge: u32) -> (u32, u32) {
    // The stored picture is 4000x3000; orientations 5 to 8 swap the axes.
    if orientation >= 5 {
        (edge * 3 / 4, edge)
    } else {
        (edge, edge * 3 / 4)
    }
}

fn check_result(
    decoder: &Decoder,
    job: &crate::decoder::Job,
    answer: Response,
    want: (u32, u32),
    reference: &std::path::Path,
    label: &str,
) -> f64 {
    let Response::ImageResult(result) = answer else {
        panic!("{label}: {answer:?}")
    };
    let file = job.out("result.webp");
    let bytes = fs::read(&file).expect("out/result.webp");
    assert_eq!((result.width, result.height), want, "{label}: dimensions");
    assert_eq!(
        header_of(&file),
        want,
        "{label}: the file's real dimensions"
    );
    assert_eq!(result.bytes, bytes.len() as u64, "{label}: bytes");
    let chunks = webp_chunks(&bytes);
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk == "VP8 " || chunk == "VP8L"),
        "{label}: metadata chunks {chunks:?}"
    );
    let _ = decoder;
    let score = ssim(&file, reference);
    assert!(
        score >= 0.95,
        "{label}: SSIM {score} against the harness render"
    );
    score
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_5_thumbnails_and_previews_apply_every_orientation() {
    let work = workdir();
    let base = pattern_jpeg(work.path(), "base.jpg", 4000, 3000);
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let mut peer = decoder.connect();
    let mut lowest: f64 = 1.0;

    for orientation in 1..=8_u8 {
        let mut bytes = base.clone();
        let at = set_orientation_at(&bytes);
        bytes[at] = orientation;
        let source = work.path().join(format!("o{orientation}.jpg"));
        fs::write(&source, &bytes).expect("write the oriented file");
        for edge in [1440_u32, 320] {
            let reference = work.path().join(format!("ref-{orientation}-{edge}.webp"));
            reference_thumbnail(&source, edge, &reference);
            let job = decoder.job(&bytes);
            let answer = peer.call(render(&job, edge, photo_limits()), WAIT);
            let label = format!("orientation {orientation} at {edge}");
            lowest = lowest.min(check_result(
                &decoder,
                &job,
                answer,
                expected(orientation, edge),
                &reference,
                &label,
            ));
        }
    }
    record(
        "IT-10-5",
        &format!("16 JPEG renders (orientation 1-8 at 1440 and 320): lowest SSIM {lowest:.4}"),
    );
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_5_heic_renders_through_the_image_decode_and_applies_irot() {
    let work = workdir();
    let heic = pattern_heic(work.path(), "p.heic", 4000, 3000);
    let source = work.path().join("p.heic");
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let mut peer = decoder.connect();

    for edge in [1440_u32, 320] {
        let reference = work.path().join(format!("ref-{edge}.webp"));
        reference_thumbnail(&source, edge, &reference);
        let job = decoder.job(&heic);
        let answer = peer.call(render(&job, edge, photo_limits()), WAIT);
        let score = check_result(
            &decoder,
            &job,
            answer,
            (edge, edge * 3 / 4),
            &reference,
            &format!("heic at {edge}"),
        );
        record(
            "IT-10-5",
            &format!("heic 4000x3000 at {edge}: SSIM {score:.4}"),
        );
    }
    let rotated = decoder.job(&rotated_heic());
    let answer = peer.call(render(&rotated, 320, photo_limits()), WAIT);
    let Response::ImageResult(result) = answer else {
        panic!("rotated heic: {answer:?}")
    };
    assert_eq!(
        (result.width, result.height),
        (64, 96),
        "96x64 with irot 270 is 64x96 upright"
    );
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_6_a_picture_one_pixel_over_the_limit_is_refused_from_its_header() {
    let work = workdir();
    let fits = work.path().join("fits.png");
    let over = work.path().join("over.png");
    run_tool(
        "vips",
        &["black", text(&fits), "1000", "1000", "--bands", "3"],
    );
    run_tool(
        "vips",
        &["black", text(&over), "1001", "1000", "--bands", "3"],
    );
    assert_eq!(
        (header_of(&fits), header_of(&over)),
        ((1000, 1000), (1001, 1000))
    );
    let mut decoder = Decoder::start(
        &release_image(),
        Flavor::Interactive,
        &[("SUNNYCLOUD_PHOTO_MAX_PIXELS", "1000000")],
    );
    let before = decoder.state();
    let ok_job = decoder.job(&fs::read(fits).expect("png"));
    let over_job = decoder.job(&fs::read(over).expect("png"));
    let mut peer = decoder.connect();

    let rendered = peer.call(render(&ok_job, 320, photo_limits()), WAIT);
    let waiter = decoder.waiter();
    let started = Instant::now();
    let refused = peer.call(render(&over_job, 320, photo_limits()), WAIT);
    let took = started.elapsed();

    assert!(matches!(rendered, Response::ImageResult(_)), "{rendered:?}");
    assert_eq!(
        refused,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Pixels
        })
    );
    assert!(took < Duration::from_secs(1), "refused after {took:?}");
    assert_eq!(
        waiter
            .stopped_within(Duration::from_secs(5))
            .map(|(code, _)| code),
        Some(3),
        "hostile-class exit"
    );
    decoder.wait_restarted(&before, WAIT).hello();
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_6_a_30000_by_30000_png_is_refused_without_a_decode() {
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let job = decoder.job(&png_header_only(30_000, 30_000));
    let mut peer = decoder.connect();
    let watch = RssWatch::start(decoder.runner_pid());
    let waiter = decoder.waiter();

    let started = Instant::now();
    let answer = peer.call(render(&job, 320, photo_limits()), WAIT);
    let took = started.elapsed();
    let exited = waiter.stopped_within(Duration::from_secs(5));
    let peak = watch.peak();

    assert_eq!(
        answer,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Pixels
        })
    );
    assert!(took < Duration::from_secs(2), "refused after {took:?}");
    assert!(peak < 200 * MIB, "runner peak RSS {} MiB", peak / MIB);
    assert_eq!(exited.map(|(code, _)| code), Some(3));
    record(
        "IT-10-6",
        &format!(
            "30000x30000 png: refused in {took:?}, runner peak RSS {} MiB",
            peak / MIB
        ),
    );
    decoder.wait_restarted(&before, WAIT).hello();
    decoder.finish();
}
