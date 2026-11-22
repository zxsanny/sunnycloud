//! Reference renders and measurements made with the host's own tools (vips, ffmpeg), never with
//! the runner's report: shared by the host-process cases and the container suite.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::media::run_tool;

pub fn text(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

/// The picture `testsrc2` draws, as a PNG: structured, so a wrong rotation or crop changes the SSIM.
pub fn pattern_png(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let path = dir.join(name);
    let source = format!("testsrc2=s={width}x{height}");
    run_tool(
        "ffmpeg",
        &[
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &source,
            "-frames:v",
            "1",
            text(&path),
        ],
    );
    path
}

/// The reference render of `source` at `edge` made by the harness's own libvips: the same operation
/// the runner is specified to run (shrink only, sRGB, metadata dropped, orientation applied).
pub fn reference_thumbnail(source: &Path, edge: u32, out: &Path) {
    let edge_text = edge.to_string();
    let target = format!("{}[Q=80,keep=none]", text(out));
    run_tool(
        "vips",
        &[
            "thumbnail",
            text(source),
            &target,
            &edge_text,
            "--height",
            &edge_text,
            "--size",
            "down",
            "--export-profile",
            "srgb",
        ],
    );
}

/// SSIM (all planes) of two images, measured by ffmpeg's `ssim` filter.
pub fn ssim(a: &Path, b: &Path) -> f64 {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-i",
            text(a),
            "-i",
            text(b),
            "-lavfi",
            "ssim",
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("ffmpeg ssim");
    assert!(
        output.status.success(),
        "ssim of {} and {}",
        a.display(),
        b.display()
    );
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    let all = log
        .lines()
        .rev()
        .find_map(|line| line.split("All:").nth(1))
        .unwrap_or_else(|| panic!("no SSIM line in {log}"));
    all.split_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("no SSIM value in {all}"))
}

/// The four-character names of the top-level chunks of a RIFF/WebP file.
pub fn webp_chunks(bytes: &[u8]) -> Vec<String> {
    assert!(
        bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "not a WebP"
    );
    let mut chunks = Vec::new();
    let mut at = 12;
    while at + 8 <= bytes.len() {
        chunks.push(String::from_utf8_lossy(&bytes[at..at + 4]).into_owned());
        let size =
            u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("four bytes")) as usize;
        at += 8 + size + (size & 1);
    }
    chunks
}

/// Prints a recorded value and appends it to `decoder-suite-records.txt` in the target directory.
pub fn record(id: &str, text: &str) {
    eprintln!("RECORDED {id}: {text}");
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("decoder-suite-records.txt");
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    writeln!(file, "{id}: {text}").expect("write the record");
}
