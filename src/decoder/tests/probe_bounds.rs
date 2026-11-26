//! AZ-3858 AC-1 and AC-2 through the real runner binary: a photo probe is a header read. The
//! dimensions equal what the harness's own `vipsheader` reads from the file. The latency bounds
//! of the spec (50 ms for 12 MP, 100 ms for the 900 MP header) are asserted on the container as
//! deployed (IT-10-4 in the container suite). Here the runner shares the host's process table:
//! every process-group check of the guard reads all of it (about 20 ms for a thousand processes),
//! which adds the same fixed cost to every probe. So this case records the times and asserts what
//! a decode would break: a 12 MP or 900 MP probe is no slower than the probe of a 640x480 picture
//! by more than 150 ms (best of five each). The memory bound is the spec's: the runner grows by
//! under 50 MiB.
mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::media::{MediaRunner, header_of, png_header_only, probe, run_tool};
use common::reference::{pattern_png, record, text};
use media_shared::decoder_protocol::{DecodeError, Response};
use media_shared::policy::{MediaKind, Probe};
use tempfile::TempDir;

const MIB: u64 = 1 << 20;
const PROBES: usize = 5;
/// What reading the pixels of a 12 MP picture would add to a header read, with margin.
const DECODE_COST: Duration = Duration::from_millis(150);

/// The peak resident size of one process (`VmHWM`), in bytes.
fn peak_resident(pid: u32) -> u64 {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).expect("read the status");
    let kib: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse().ok())
        .expect("VmHWM in the status");
    kib * 1024
}

fn dimensions(facts: &Probe) -> (u32, u32) {
    let video = facts.video.as_ref().expect("a photo probe has dimensions");
    (video.width, video.height)
}

#[test]
fn a_photo_probe_is_a_header_read_that_matches_the_file_is_fast_and_does_not_grow_the_runner() {
    let work = TempDir::new().expect("harness work directory");
    let png = pattern_png(work.path(), "p.png", 4000, 3000);
    let jpeg = work.path().join("p.jpg");
    let webp = work.path().join("p.webp");
    let heic = work.path().join("p.heic");
    let gif = work.path().join("p.gif");
    run_tool("vips", &["jpegsave", text(&png), text(&jpeg), "--Q", "85"]);
    run_tool("vips", &["webpsave", text(&png), text(&webp)]);
    run_tool("vips", &["heifsave", text(&png), text(&heic), "--Q", "50"]);
    let small = pattern_png(work.path(), "small.png", 640, 480);
    run_tool("vips", &["gifsave", text(&small), text(&gif)]);
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let baseline = peak_resident(runner.pid());
    let mut best: Vec<(&str, Duration)> = Vec::new();

    for (name, path, twelve_mp) in [
        ("jpeg 12 MP", jpeg, true),
        ("heic 12 MP", heic, true),
        ("png 12 MP", png, true),
        ("webp 12 MP", webp, true),
        ("gif 640x480", gif, false),
    ] {
        let manifest = header_of(&path);
        let job = runner.job(&fs::read(&path).expect("read the photo"));
        let mut times = Vec::new();
        let mut last = None;
        for _ in 0..PROBES {
            let started = Instant::now();
            last = Some(probe(&mut client, &job));
            times.push(started.elapsed());
        }
        times.sort();
        let Some(Response::Probe(facts)) = last else {
            panic!("{name}: no probe answer");
        };

        assert_eq!(facts.kind, MediaKind::Photo, "{name}");
        assert_eq!(
            dimensions(&facts),
            manifest,
            "{name}: the dimensions of the file"
        );
        best.push((name, times[0]));
        record(
            "AZ-3858 AC-1 host",
            &format!(
                "{name}: {}x{}, probe best {:?}, median {:?}{}",
                manifest.0,
                manifest.1,
                times[0],
                times[PROBES / 2],
                if twelve_mp {
                    " (product bound 50 ms)"
                } else {
                    ""
                }
            ),
        );
    }
    let huge = runner.job(&png_header_only(30_000, 30_000));
    let mut times = Vec::new();
    for _ in 0..PROBES {
        let started = Instant::now();
        let answer = probe(&mut client, &huge);
        times.push(started.elapsed());
        let Response::Probe(facts) = answer else {
            panic!("30000x30000 png: {answer:?}");
        };
        assert_eq!(dimensions(&facts), (30_000, 30_000));
    }
    times.sort();
    let small = best
        .iter()
        .find(|(name, _)| name.starts_with("gif"))
        .expect("the small case")
        .1;
    for (name, took) in best.iter().chain([&("png 30000x30000", times[0])]) {
        assert!(
            *took < small + DECODE_COST,
            "{name}: {took:?} against {small:?} for 640x480"
        );
    }
    record(
        "AZ-3858 AC-1 host",
        &format!(
            "png 30000x30000 header only: probe best {:?} (product bound 100 ms)",
            times[0]
        ),
    );
    let growth = peak_resident(runner.pid()).saturating_sub(baseline);
    assert!(
        growth < 50 * MIB,
        "the probes grew the runner by {} MiB",
        growth / MIB
    );
    record(
        "AZ-3858 AC-1 host",
        &format!(
            "runner growth over all probes {} MiB (bound 50 MiB)",
            growth / MIB
        ),
    );
}

#[test]
fn a_truncated_jpeg_is_corrupt_or_has_its_intact_header_dimensions_and_the_outcome_is_recorded() {
    let work = TempDir::new().expect("harness work directory");
    let png = pattern_png(work.path(), "t.png", 1600, 1200);
    let jpeg = work.path().join("t.jpg");
    run_tool("vips", &["jpegsave", text(&png), text(&jpeg), "--Q", "85"]);
    let bytes = fs::read(&jpeg).expect("read the JPEG");
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let cut_in_header = runner.job(&bytes[..100]);
    let cut_in_pixels = runner.job(&bytes[..bytes.len() / 2]);

    let in_header = probe(&mut client, &cut_in_header);
    let in_pixels = probe(&mut client, &cut_in_pixels);

    assert_eq!(in_header, Response::Error(DecodeError::Corrupt));
    let pixels_outcome = match &in_pixels {
        Response::Probe(facts) => {
            assert_eq!(dimensions(facts), (1600, 1200));
            "the intact header's 1600x1200".to_owned()
        }
        Response::Error(DecodeError::Corrupt) => "Corrupt".to_owned(),
        other => panic!("unexpected answer {other:?}"),
    };
    record(
        "AZ-3858 AC-2 host",
        &format!(
            "truncated JPEG: cut at 100 bytes {in_header:?}; cut in the pixel data {pixels_outcome}"
        ),
    );
}
