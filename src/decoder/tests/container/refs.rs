//! Reference outputs from the harness's own host tools (vips, ffmpeg, ffprobe on `PATH`); the
//! runner's report is never the reference.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::common::media::run_tool;

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

/// A scratch directory of the harness for generated media.
pub fn workdir() -> TempDir {
    TempDir::new().expect("harness work directory")
}

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

/// A structured JPEG saved by libvips (it writes an EXIF orientation tag that `oriented` patches).
pub fn pattern_jpeg(dir: &Path, name: &str, width: u32, height: u32) -> Vec<u8> {
    let png = pattern_png(dir, &format!("{name}.png"), width, height);
    let out = dir.join(name);
    run_tool("vips", &["jpegsave", text(&png), text(&out), "--Q", "85"]);
    std::fs::read(out).expect("read the generated JPEG")
}

/// A structured HEIC (HEVC) made by the host's libheif through `vips heifsave`.
pub fn pattern_heic(dir: &Path, name: &str, width: u32, height: u32) -> Vec<u8> {
    let png = pattern_png(dir, &format!("{name}.png"), width, height);
    let out = dir.join(name);
    run_tool("vips", &["heifsave", text(&png), text(&out), "--Q", "50"]);
    std::fs::read(out).expect("read the generated HEIC")
}

/// What the harness's own `ffprobe` reports for a file (all streams and the format), as JSON.
pub fn ffprobe_json(path: &Path) -> serde_json::Value {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
            text(path),
        ])
        .output()
        .expect("ffprobe");
    assert!(output.status.success(), "ffprobe {}", path.display());
    serde_json::from_slice(&output.stdout).expect("ffprobe json")
}

/// The first stream of `kind` (`video` or `audio`) in an ffprobe report.
pub fn first_stream<'a>(
    report: &'a serde_json::Value,
    kind: &str,
) -> Option<&'a serde_json::Value> {
    report["streams"]
        .as_array()?
        .iter()
        .find(|stream| stream["codec_type"] == kind)
}

/// The display-matrix rotation of a video stream in degrees, as ffprobe reports it.
pub fn rotation_of(stream: &serde_json::Value) -> Option<i64> {
    stream["side_data_list"]
        .as_array()?
        .iter()
        .find_map(|side| side["rotation"].as_i64())
}

/// A clip made by the host's ffmpeg: `source` is the lavfi graph, `rest` the output options.
pub fn make_clip(dir: &Path, name: &str, inputs: &[&str], rest: &[&str]) -> PathBuf {
    let path = dir.join(name);
    let mut args = vec!["-loglevel", "error", "-y"];
    args.extend_from_slice(inputs);
    args.extend_from_slice(rest);
    args.push(text(&path));
    run_tool("ffmpeg", &args);
    path
}
