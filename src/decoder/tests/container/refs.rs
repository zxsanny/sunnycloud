//! Reference outputs from the harness's own host tools (vips, ffmpeg, ffprobe on `PATH`); the
//! runner's report is never the reference.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::common::media::run_tool;
pub use crate::common::reference::{
    pattern_png, record, reference_thumbnail, ssim, text, webp_chunks,
};

/// A scratch directory of the harness for generated media.
pub fn workdir() -> TempDir {
    TempDir::new().expect("harness work directory")
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
