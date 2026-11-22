//! AZ-3857 AC-3 through the real runner binary and the host's real libvips: every EXIF
//! orientation, the HEIC set, fidelity against references the harness makes itself and the proof
//! that the output carries no EXIF, GPS or ICC data, read with the harness's own tools.
//!
//! The upright reference is the picture the stored image was made from, so a wrong rotation or
//! mirror cannot hide behind a reference made by the same code path. Nothing here asserts the
//! bytes of an encoder: libvips 8.15 (CI) and 8.18 differ in them.
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::media::{MediaRunner, header_of, image_result, limits, render, run_tool};
use common::reference::{pattern_png, record, reference_thumbnail, ssim, text, webp_chunks};
use tempfile::TempDir;

/// The spec's floor (AC-10): the same on libvips 8.15 and 8.18 because every reference is made
/// by the installed tool.
const SSIM_FLOOR: f64 = 0.95;
const MAKER: &str = "SCMAKER-SECRET";

/// The stored picture of an upright `source` that EXIF orientation `orientation` turns upright.
fn stored_for(orientation: u8, source: &Path, dir: &Path) -> PathBuf {
    let out = dir.join(format!("stored-{orientation}.png"));
    let turned = dir.join(format!("turned-{orientation}.png"));
    let (source, out_text) = (text(source), text(&out));
    match orientation {
        1 => run_tool("vips", &["copy", source, out_text]),
        2 => run_tool("vips", &["flip", source, out_text, "horizontal"]),
        3 => run_tool("vips", &["rot", source, out_text, "d180"]),
        4 => run_tool("vips", &["flip", source, out_text, "vertical"]),
        5 => {
            run_tool("vips", &["rot", source, text(&turned), "d90"]);
            run_tool("vips", &["flip", text(&turned), out_text, "horizontal"]);
        }
        6 => run_tool("vips", &["rot", source, out_text, "d270"]),
        7 => {
            run_tool("vips", &["rot", source, text(&turned), "d270"]);
            run_tool("vips", &["flip", text(&turned), out_text, "horizontal"]);
        }
        8 => run_tool("vips", &["rot", source, out_text, "d90"]),
        other => panic!("no EXIF orientation {other}"),
    }
    out
}

/// A little-endian EXIF block with an orientation, a maker string and a GPS position.
fn exif_block(orientation: u8) -> Vec<u8> {
    fn entry(out: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    let maker = format!("{MAKER}\0");
    let maker_at = 8 + 2 + 3 * 12 + 4;
    let gps_at = maker_at + maker.len().next_multiple_of(2);
    let lat_at = gps_at + 2 + 4 * 12 + 4;
    let lon_at = lat_at + 24;
    let mut tiff = b"II*\0\x08\0\0\0".to_vec();
    tiff.extend_from_slice(&3u16.to_le_bytes());
    entry(&mut tiff, 0x010F, 2, maker.len() as u32, maker_at as u32);
    entry(&mut tiff, 0x0112, 3, 1, u32::from(orientation));
    entry(&mut tiff, 0x8825, 4, 1, gps_at as u32);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(maker.as_bytes());
    tiff.resize(gps_at, 0);
    tiff.extend_from_slice(&4u16.to_le_bytes());
    entry(&mut tiff, 1, 2, 2, u32::from_le_bytes([b'N', 0, 0, 0]));
    entry(&mut tiff, 2, 5, 3, lat_at as u32);
    entry(&mut tiff, 3, 2, 2, u32::from_le_bytes([b'E', 0, 0, 0]));
    entry(&mut tiff, 4, 5, 3, lon_at as u32);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    for rational in [[50, 1], [27, 1], [1234, 100], [30, 1], [31, 1], [4321, 100]] {
        for part in rational {
            tiff.extend_from_slice(&(part as u32).to_le_bytes());
        }
    }
    let mut segment = vec![0xFF, 0xE1];
    segment.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(&tiff);
    segment
}

/// `jpeg` with its own EXIF segments replaced by `exif`, placed after the JFIF header.
fn with_exif(jpeg: &[u8], exif: &[u8]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    let mut at = 2;
    let mut inserted = false;
    while at + 4 <= jpeg.len() && jpeg[at] == 0xFF && jpeg[at + 1] != 0xDA {
        let marker = jpeg[at + 1];
        let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        if marker != 0xE0 && !inserted {
            out.extend_from_slice(exif);
            inserted = true;
        }
        if marker != 0xE1 {
            out.extend_from_slice(&jpeg[at..at + 2 + length]);
        }
        at += 2 + length;
    }
    out.extend_from_slice(&jpeg[at..]);
    out
}

/// A JPEG of the stored picture with an ICC profile, a GPS position and a maker string.
fn rich_jpeg(stored: &Path, orientation: u8, dir: &Path) -> (PathBuf, Vec<u8>) {
    let plain = dir.join(format!("plain-{orientation}.jpg"));
    run_tool(
        "vips",
        &[
            "jpegsave",
            text(stored),
            text(&plain),
            "--Q",
            "95",
            "--profile",
            "srgb",
        ],
    );
    let bytes = with_exif(
        &fs::read(&plain).expect("read the JPEG"),
        &exif_block(orientation),
    );
    let path = dir.join(format!("rich-{orientation}.jpg"));
    fs::write(&path, &bytes).expect("write the JPEG");
    (path, bytes)
}

/// The metadata fields libvips itself lists for `path` (`vipsheader -a`).
fn header_fields(path: &Path) -> String {
    let output = Command::new("vipsheader")
        .arg("-a")
        .arg(path)
        .output()
        .expect("vipsheader");
    assert!(output.status.success(), "{}", path.display());
    String::from_utf8_lossy(&output.stdout).to_lowercase()
}

fn carries_metadata(fields: &str) -> bool {
    ["exif", "gps", "icc-profile", "xmp", "iptc", "scmaker"]
        .iter()
        .any(|needle| fields.contains(needle))
}

/// The source really carries what the render must drop, as libvips reads it.
fn assert_source_carries_metadata(path: &Path, bytes: &[u8]) {
    let fields = header_fields(path);
    assert!(fields.contains("gps"), "{fields}");
    assert!(fields.contains("icc-profile"), "{fields}");
    assert!(bytes.windows(MAKER.len()).any(|w| w == MAKER.as_bytes()));
}

fn assert_stripped(result: &Path, label: &str) {
    let chunks = webp_chunks(&fs::read(result).expect("result"));
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk == "VP8 " || chunk == "VP8L"),
        "{label}: chunks {chunks:?}"
    );
    let fields = header_fields(result);
    assert!(!carries_metadata(&fields), "{label}: {fields}");
    let bytes = fs::read(result).expect("result");
    assert!(
        !bytes.windows(MAKER.len()).any(|w| w == MAKER.as_bytes()),
        "{label}: the maker string is in the output"
    );
}

#[test]
fn every_exif_orientation_comes_out_upright_with_the_edge_the_dimensions_and_no_metadata() {
    let work = TempDir::new().expect("harness work directory");
    let upright = pattern_png(work.path(), "upright.png", 1600, 1200);
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let mut lowest: f64 = 1.0;

    for orientation in 1..=8u8 {
        let stored = stored_for(orientation, &upright, work.path());
        let (path, bytes) = rich_jpeg(&stored, orientation, work.path());
        assert_source_carries_metadata(&path, &bytes);
        let swapped = orientation >= 5;
        assert_eq!(
            header_of(&path),
            if swapped { (1200, 1600) } else { (1600, 1200) },
            "orientation {orientation}: the stored picture"
        );
        for edge in [320u32, 1440] {
            let label = format!("orientation {orientation} at {edge}");
            let job = runner.job(&bytes);
            let reply = image_result(render(&mut client, &job, edge, limits()));
            let result = job.join("out/result.webp");
            let upright_reference = work.path().join(format!("up-{orientation}-{edge}.webp"));
            let same_tool_reference = work.path().join(format!("same-{orientation}-{edge}.webp"));
            reference_thumbnail(&upright, edge, &upright_reference);
            reference_thumbnail(&path, edge, &same_tool_reference);

            assert_eq!(
                (reply.width, reply.height),
                (edge, edge * 3 / 4),
                "{label}: the long edge and the upright shape"
            );
            assert_eq!(header_of(&result), (reply.width, reply.height), "{label}");
            assert_stripped(&result, &label);
            let against_upright = ssim(&result, &upright_reference);
            let against_same_tool = ssim(&result, &same_tool_reference);
            assert!(against_upright >= SSIM_FLOOR, "{label}: {against_upright}");
            assert!(
                against_same_tool >= SSIM_FLOOR,
                "{label}: {against_same_tool}"
            );
            lowest = lowest.min(against_upright).min(against_same_tool);
        }
    }

    record(
        "AZ-3857 AC-3 host",
        &format!(
            "EXIF orientation 1-8 at 320 and 1440, lowest SSIM {lowest:.4} (floor {SSIM_FLOOR})"
        ),
    );
}

#[test]
fn a_set_of_heic_photos_renders_at_both_edges_close_to_the_references_with_no_metadata() {
    let work = TempDir::new().expect("harness work directory");
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let mut lowest: f64 = 1.0;
    let sizes = [(4000u32, 3000u32), (3000, 4000), (2400, 1800), (1600, 1200)];

    for (index, (width, height)) in sizes.into_iter().enumerate() {
        let upright = pattern_png(work.path(), &format!("heic-src-{index}.png"), width, height);
        let heic = work.path().join(format!("set-{index}.heic"));
        run_tool(
            "vips",
            &["heifsave", text(&upright), text(&heic), "--Q", "80"],
        );
        let bytes = fs::read(&heic).expect("read the HEIC");
        for edge in [320u32, 1440] {
            let label = format!("heic {width}x{height} at {edge}");
            let job = runner.job(&bytes);
            let reply = image_result(render(&mut client, &job, edge, limits()));
            let result = job.join("out/result.webp");
            let reference = work.path().join(format!("heic-ref-{index}-{edge}.webp"));
            reference_thumbnail(&upright, edge, &reference);

            let long = width.max(height);
            let scale = f64::from(edge) / f64::from(long);
            let expected = (
                (f64::from(width) * scale).round() as u32,
                (f64::from(height) * scale).round() as u32,
            );
            assert_eq!((reply.width, reply.height), expected, "{label}");
            assert_stripped(&result, &label);
            let score = ssim(&result, &reference);
            assert!(score >= SSIM_FLOOR, "{label}: SSIM {score}");
            lowest = lowest.min(score);
        }
    }

    record(
        "AZ-3857 AC-3 host",
        &format!(
            "4 HEIC photos (vips heifsave Q80) at 320 and 1440, lowest SSIM against the PNG they came from {lowest:.4}"
        ),
    );
}
