//! Measurements of a clip by the host's own ffmpeg and ffprobe (bitrate windows, luma, beeps,
//! reference frames); the runner's report is never the reference.

use std::path::Path;
use std::process::Command;

use super::media::run_tool;

fn text(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

/// The most video bits in any window of one second that starts at a packet, and the average
/// bits per second over the whole stream.
pub fn bitrate_windows(path: &Path) -> (u64, f64) {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=pts_time,size",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    assert!(output.status.success());
    let mut packets: Vec<(f64, u64)> = String::from_utf8(output.stdout)
        .expect("utf8")
        .lines()
        .filter_map(|line| {
            let (time, size) = line.split_once(',')?;
            Some((time.parse().ok()?, size.parse().ok()?))
        })
        .collect();
    packets.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(!packets.is_empty(), "no packets in {}", path.display());
    let worst = packets
        .iter()
        .map(|(start, _)| {
            packets
                .iter()
                .filter(|(time, _)| time >= start && *time < start + 1.0)
                .map(|(_, size)| size * 8)
                .sum::<u64>()
        })
        .max()
        .expect("a window");
    let total: u64 = packets.iter().map(|(_, size)| size * 8).sum();
    let span = packets.last().expect("last").0 - packets[0].0;
    (worst, total as f64 / span.max(0.1))
}

/// Mean 8-bit luma code of the first frame of `path` after `filter` (`null` for none).
pub fn mean_luma(path: &Path, filter: &str) -> f64 {
    let graph = format!("{filter},format=gray");
    let output = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-i",
            text(path),
            "-vf",
            &graph,
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .expect("ffmpeg luma");
    assert!(output.status.success(), "luma of {}", path.display());
    assert!(!output.stdout.is_empty(), "no frame in {}", path.display());
    output.stdout.iter().map(|&b| f64::from(b)).sum::<f64>() / output.stdout.len() as f64
}

/// The first frame of `path` through `filter` as a PNG (ffmpeg's default autorotate applies).
pub fn first_frame(path: &Path, filter: &str, out: &Path) {
    run_tool(
        "ffmpeg",
        &[
            "-loglevel",
            "error",
            "-y",
            "-i",
            text(path),
            "-vf",
            filter,
            "-frames:v",
            "1",
            text(out),
        ],
    );
}

/// Times in seconds at which sound starts after at least half a second of silence, as ffmpeg's
/// `silencedetect` reports them for the first audio stream; only the first `count` are kept, because
/// the filter also reports the end of the stream as one more `silence_end`.
pub fn beep_onsets(path: &Path, count: usize) -> Vec<f64> {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-i",
            text(path),
            "-map",
            "0:a:0",
            "-af",
            "silencedetect=noise=-40dB:d=0.5",
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("ffmpeg silencedetect");
    assert!(
        output.status.success(),
        "silencedetect of {}",
        path.display()
    );
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| line.split("silence_end: ").nth(1))
        .filter_map(|rest| rest.split_whitespace().next()?.parse().ok())
        .take(count)
        .collect()
}

/// Mean of the per-frame average luma (`signalstats` YAVG) over the whole clip after `filter`.
pub fn mean_yavg(path: &Path, filter: &str) -> f64 {
    let graph = format!("{filter},signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-");
    let output = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-i",
            text(path),
            "-vf",
            &graph,
            "-an",
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("ffmpeg signalstats");
    assert!(output.status.success(), "signalstats of {}", path.display());
    let values: Vec<f64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split("YAVG=").nth(1)?.trim().parse().ok())
        .collect();
    assert!(
        !values.is_empty(),
        "no YAVG in the output for {}",
        path.display()
    );
    values.iter().sum::<f64>() / values.len() as f64
}

/// Average PSNR in dB of two images of the same size, measured by ffmpeg's `psnr` filter (all
/// planes; infinity for identical images is reported as 100).
pub fn psnr(a: &Path, b: &Path) -> f64 {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-i",
            text(a),
            "-i",
            text(b),
            "-lavfi",
            "psnr",
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("ffmpeg psnr");
    assert!(
        output.status.success(),
        "psnr of {} and {}",
        a.display(),
        b.display()
    );
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    let average = log
        .lines()
        .rev()
        .find_map(|line| line.split("average:").nth(1))
        .unwrap_or_else(|| panic!("no PSNR line in {log}"));
    match average.split_whitespace().next() {
        Some("inf") => 100.0,
        Some(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("not a number: {value}")),
        None => panic!("no PSNR value in {average}"),
    }
}
