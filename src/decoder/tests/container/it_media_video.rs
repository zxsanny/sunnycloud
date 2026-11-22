//! IT-10-7 and the video probe on the release image (real ffmpeg 9.1, libvips 8.18.7): the poster is the frame
//! nearest the requested time, an HLG poster is tone mapped (SSIM recorded, G-10-6), and `probe` reports the
//! facts the harness's own ffprobe (8.x on the host) reports.

use std::fs;
use std::time::Duration;

use media_shared::decoder_protocol::Response;
use media_shared::policy::MediaKind;

use crate::common::media::{Media, mean_of};
use crate::decoder::{Decoder, Flavor, release_image};
use crate::peer::{photo_limits, probe, render, render_at};
use crate::refs::{
    ffprobe_json, first_stream, make_clip, pattern_jpeg, record, reference_thumbnail, rotation_of,
    ssim, text, workdir,
};

const WAIT: Duration = Duration::from_secs(60);

/// Frame `N` of the clip is a flat grey of luma `8 * N` at 10 fps; the picture shows which frame was decoded.
fn stepped_clip(dir: &std::path::Path) -> std::path::PathBuf {
    let graph = "color=c=black:s=640x360:r=10:d=3,format=yuv420p,geq=lum='8*N':cb=128:cr=128";
    make_clip(
        dir,
        "stepped.mp4",
        &["-f", "lavfi", "-i", graph],
        &["-pix_fmt", "yuv420p", "-movflags", "+faststart"],
    )
}

/// The RGB value a luma code of the limited range decodes to (BT.601, the default for unmarked yuv420p).
fn rgb_of_luma(luma: f64) -> f64 {
    (luma - 16.0) * 255.0 / 219.0
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_7_a_poster_is_the_frame_nearest_the_requested_time() {
    let work = workdir();
    let clip = stepped_clip(work.path());
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let mut peer = decoder.connect();
    let bytes = fs::read(&clip).expect("clip");
    let mut means = Vec::new();

    for (at_ms, frame) in [(1000_u64, 10_u32), (900, 9)] {
        let job = decoder.job(&bytes);
        let answer = peer.call(render_at(&job, 320, at_ms), WAIT);
        let Response::ImageResult(result) = answer else {
            panic!("poster at {at_ms}: {answer:?}")
        };
        assert_eq!(
            (result.width, result.height),
            (320, 180),
            "a 640x360 frame comes out as 320 px WebP"
        );
        let mean = mean_of(&job.out("result.webp"));
        let want = rgb_of_luma(f64::from(8 * frame));
        // One frame is 8 luma codes (7.3 in RGB): this tolerance tells the frame from its neighbours.
        assert!(
            (mean - want).abs() < 3.5,
            "poster at {at_ms} ms has mean {mean:.1}, frame {frame} is {want:.1}"
        );
        means.push(mean);
    }
    assert!(
        (means[0] - means[1]).abs() > 4.0,
        "the posters of 1000 and 900 ms are different frames: {means:?}"
    );

    // `poster_at_ms` is ignored for images
    let photo = pattern_jpeg(work.path(), "p.jpg", 800, 600);
    let (first, second) = (decoder.job(&photo), decoder.job(&photo));
    let plain = peer.call(render(&first, 320, photo_limits()), WAIT);
    let with_offset = peer.call(render_at(&second, 320, 5000), WAIT);
    assert_eq!(plain, with_offset);
    record(
        "IT-10-7",
        &format!(
            "posters at 1000/900 ms: mean {:.1}/{:.1} (expected 74.5/65.2)",
            means[0], means[1]
        ),
    );
    decoder.finish();
}

const HDR_CHAIN: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,\
zscale=t=bt709:m=bt709:r=tv,format=yuv420p,format=rgb24";

/// The harness render of a poster: one frame at 1 s through `chain`, then the same WebP step as the runner.
fn harness_poster(
    work: &std::path::Path,
    clip: &std::path::Path,
    chain: &str,
    name: &str,
) -> std::path::PathBuf {
    let png = make_clip(
        work,
        &format!("{name}.png"),
        &["-ss", "1.0", "-i", text(clip)],
        &["-frames:v", "1", "-vf", chain],
    );
    let webp = work.join(format!("{name}.webp"));
    reference_thumbnail(&png, 320, &webp);
    webp
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_7_an_hlg_poster_is_tone_mapped_and_its_ssim_is_recorded() {
    let work = workdir();
    let media = Media::new();
    let clip = work.path().join("hlg.mp4");
    fs::write(&clip, media.hlg_mp4()).expect("hlg clip");
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let job = decoder.job(&fs::read(&clip).expect("clip"));
    let mut peer = decoder.connect();

    let answer = peer.call(render_at(&job, 320, 1000), WAIT);

    let Response::ImageResult(result) = answer else {
        panic!("hlg poster: {answer:?}")
    };
    assert_eq!((result.width, result.height), (320, 240));
    let poster = job.out("result.webp");
    let tone_mapped = harness_poster(work.path(), &clip, HDR_CHAIN, "ref-hable");
    let untouched = harness_poster(work.path(), &clip, "format=rgb24", "ref-plain");
    let (score, control) = (ssim(&poster, &tone_mapped), ssim(&poster, &untouched));
    // G-10-6: recorded, not asserted here. The control shows that the tone map is what the score measures.
    record(
        "IT-10-7 HLG",
        &format!(
            "synthetic 320x240 HEVC Main10 HLG clip, poster at 1 s: SSIM vs the harness chain {score:.4} (0.95 wanted), vs no tone map {control:.4}; mean luma poster {:.1}, harness {:.1}, plain {:.1}",
            mean_of(&poster),
            mean_of(&tone_mapped),
            mean_of(&untouched)
        ),
    );
    assert!(score.is_finite() && control.is_finite());
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_7_probe_of_video_files_equals_the_harness_ffprobe() {
    let work = workdir();
    let media = Media::new();
    let av = make_clip(
        work.path(),
        "av.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=d=2:s=320x240:r=10",
            "-f",
            "lavfi",
            "-i",
            "sine=d=2",
        ],
        &[
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ],
    );
    let files = [
        (
            "h264 moov at end",
            work.path().join("end.mp4"),
            media.mp4(2),
            false,
        ),
        (
            "h264 faststart",
            work.path().join("fast.mp4"),
            media.faststart_mp4(2),
            true,
        ),
        (
            "h264 + aac",
            av.clone(),
            fs::read(&av).expect("av clip"),
            false,
        ),
        ("quicktime", work.path().join("q.mov"), media.mov(), false),
        (
            "hevc 10-bit HLG",
            work.path().join("hlg.mp4"),
            media.hlg_mp4(),
            false,
        ),
        (
            "rotated 90",
            work.path().join("rot.mp4"),
            media.rotated_mp4(),
            false,
        ),
    ];
    let mut decoder = Decoder::start(&release_image(), Flavor::General, &[]);
    let mut peer = decoder.connect();

    for (name, path, bytes, moov_first) in files {
        fs::write(&path, &bytes).expect("clip");
        let report = ffprobe_json(&path);
        let video = first_stream(&report, "video").expect("a video stream");
        let job = decoder.job(&bytes);

        let answer = peer.call(probe(&job), WAIT);

        let Response::Probe(probed) = answer else {
            panic!("{name}: {answer:?}")
        };
        assert_eq!(probed.kind, MediaKind::Video, "{name}");
        let stream = probed.video.as_ref().expect("video facts");
        assert_eq!(
            stream.codec.as_deref(),
            video["codec_name"].as_str(),
            "{name}: codec"
        );
        assert_eq!(
            u64::from(stream.width),
            video["width"].as_u64().expect("width"),
            "{name}: width"
        );
        assert_eq!(
            u64::from(stream.height),
            video["height"].as_u64().expect("height"),
            "{name}: height"
        );
        assert_eq!(
            stream.pix_fmt.as_deref(),
            video["pix_fmt"].as_str(),
            "{name}: pix_fmt"
        );
        assert_eq!(
            stream.primaries.as_deref(),
            video["color_primaries"].as_str(),
            "{name}: primaries"
        );
        assert_eq!(
            stream.transfer.as_deref(),
            video["color_transfer"].as_str(),
            "{name}: transfer"
        );
        assert_eq!(
            stream.matrix.as_deref(),
            video["color_space"].as_str(),
            "{name}: matrix"
        );
        assert_eq!(
            stream.rotation.map(i64::from),
            rotation_of(video),
            "{name}: rotation"
        );
        assert_eq!(
            probed.audio.as_ref().map(|audio| audio.codec.as_str()),
            first_stream(&report, "audio").and_then(|a| a["codec_name"].as_str()),
            "{name}: audio"
        );
        assert_eq!(probed.moov_first, moov_first, "{name}: moov_first");
        let seconds: f64 = report["format"]["duration"]
            .as_str()
            .expect("duration")
            .parse()
            .expect("seconds");
        let reported = probed.duration_ms.expect("duration_ms") as f64;
        assert!(
            (reported - seconds * 1000.0).abs() <= 100.0,
            "{name}: duration {reported} vs {seconds}"
        );
        record("IT-10-7 probe", &format!("{name}: {probed:?}"));
    }
    decoder.finish();
}
