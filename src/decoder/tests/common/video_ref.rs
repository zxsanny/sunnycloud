//! Clips and measurements for the transcode proofs, made by the host's own ffmpeg and ffprobe
//! at test time: the runner's report is never the reference, and no exact encoder bytes are
//! compared (the CI tools are not the host's).

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use super::media::run_tool;

fn text(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

/// The tone-mapping chain of gate S5 (10 section 13.2), written out here so that the reference
/// does not come from the runner's code.
pub const HDR_CHAIN: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p";

/// A directory of generated clips.
pub struct Clips {
    dir: TempDir,
}

impl Clips {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("clip dir"),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn make(&self, name: &str, inputs: &[&str], rest: &[&str]) -> PathBuf {
        let path = self.path(name);
        let mut args = vec!["-loglevel", "error", "-y"];
        args.extend_from_slice(inputs);
        args.extend_from_slice(rest);
        args.push(text(&path));
        run_tool("ffmpeg", &args);
        path
    }

    /// H.264 of the `testsrc2` pattern (asymmetric, so a wrong rotation shows), 10 fps, with a
    /// mono sine as AAC when `audio`. The index is at the end.
    pub fn sdr(&self, name: &str, width: u32, height: u32, seconds: u32, audio: bool) -> PathBuf {
        let video = format!("testsrc2=s={width}x{height}:r=10:d={seconds}");
        let sine = format!("sine=frequency=440:duration={seconds}");
        let mut inputs = vec!["-f", "lavfi", "-i", &video];
        let mut rest = vec![
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ];
        if audio {
            inputs.extend(["-f", "lavfi", "-i", &sine]);
            rest.extend(["-c:a", "aac"]);
        }
        self.make(name, &inputs, &rest)
    }

    /// `sdr` with a display-matrix rotation of `degrees`, stream copied.
    pub fn rotated(&self, name: &str, width: u32, height: u32, degrees: i32) -> PathBuf {
        let plain = self.sdr(&format!("plain-{name}"), width, height, 2, true);
        let angle = degrees.to_string();
        self.make(
            name,
            &["-display_rotation", &angle, "-i", text(&plain)],
            &["-c", "copy"],
        )
    }

    /// H.264 whose pixels are coded with the BT.601 matrix and tagged `smpte170m`, as an SD source
    /// is (`bt709` false), or coded and tagged BT.709 (`standard` true).
    pub fn sdr_in(&self, name: &str, bt709: bool) -> PathBuf {
        let (matrix, tag) = if bt709 {
            ("bt709", "bt709")
        } else {
            ("bt601", "smpte170m")
        };
        let graph = format!(
            "scale=out_color_matrix={matrix}:out_range=tv,format=yuv420p,setparams=colorspace={tag}:color_primaries={tag}:color_trc={tag}"
        );
        self.make(
            name,
            &["-f", "lavfi", "-i", "testsrc2=s=640x360:r=10:d=2"],
            &[
                "-vf",
                &graph,
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-colorspace",
                tag,
                "-color_primaries",
                tag,
                "-color_trc",
                tag,
            ],
        )
    }

    /// 321x241 H.264 (4:4:4, because 4:2:0 needs even sides).
    pub fn odd_sided(&self) -> PathBuf {
        self.make(
            "odd.mp4",
            &["-f", "lavfi", "-i", "testsrc2=s=321x241:r=10:d=2"],
            &[
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv444p",
            ],
        )
    }

    /// 10-bit HEVC tagged HLG, BT.2020, with AAC.
    pub fn hlg(&self, width: u32, height: u32) -> PathBuf {
        let video = format!("testsrc2=s={width}x{height}:r=10:d=2");
        self.make(
            "hlg.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                &video,
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
            ],
            &[
                "-vf",
                "format=yuv420p10le",
                "-c:v",
                "libx265",
                "-preset",
                "ultrafast",
                "-x265-params",
                "colorprim=bt2020:transfer=arib-std-b67:colormatrix=bt2020nc:log-level=error",
                "-c:a",
                "aac",
            ],
        )
    }

    /// 1080p H.264 with AAC at about `kbps` kbit/s, like a phone clip (DS-05 base is 18 to 20
    /// Mbit/s): noise on the test pattern gives the encoder something to spend the rate on.
    pub fn phone_1080p(&self, seconds: u32, kbps: u32) -> PathBuf {
        let video =
            format!("testsrc2=s=1920x1080:r=30:d={seconds},noise=alls=12:allf=t,format=yuv420p");
        let rate = format!("{kbps}k");
        self.make(
            "phone.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                &video,
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
            ],
            &[
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-b:v",
                &rate,
                "-maxrate",
                &rate,
                "-bufsize",
                &rate,
                "-c:a",
                "aac",
                "-shortest",
            ],
        )
    }

    /// Variable frame rate video (frames dropped in a pattern, timestamps kept) and a 1 kHz
    /// beep of 0.3 s at 1.0, 3.0 and 5.0 s, so that an audio shift shows in the onsets.
    pub fn vfr_beeps(&self) -> PathBuf {
        let audio = "aevalsrc=exprs='sin(2*PI*1000*t)*(between(t,1,1.3)+between(t,3,3.3)+between(t,5,5.3))':d=6:s=44100";
        let video = "testsrc2=s=640x360:r=30:d=6,select='lt(mod(n,7),3+mod(floor(n/30),3))'";
        self.make(
            "vfr.mp4",
            &["-f", "lavfi", "-i", video, "-f", "lavfi", "-i", audio],
            &[
                "-fps_mode",
                "vfr",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ],
        )
    }

    /// High-entropy H.264 at a high bitrate: the encoder has to hold `-maxrate` down.
    pub fn noisy(&self, width: u32, height: u32, seconds: u32) -> PathBuf {
        let video = format!(
            "testsrc2=s={width}x{height}:r=30:d={seconds},noise=alls=30:allf=t+u,format=yuv420p"
        );
        self.make(
            &format!("noisy-{width}x{height}-{seconds}.mp4"),
            &["-f", "lavfi", "-i", &video],
            &["-c:v", "libx264", "-preset", "ultrafast", "-crf", "14"],
        )
    }

    /// H.264 and AAC with the index at the end, a title and a GPS-style location tag (global
    /// and on the video stream), a `mov_text` subtitle stream and one chapter.
    pub fn tagged(&self) -> PathBuf {
        let subtitle = self.path("sub.srt");
        std::fs::write(&subtitle, "1\n00:00:00,000 --> 00:00:01,000\nhello\n").expect("srt");
        let meta = self.path("meta.txt");
        std::fs::write(
            &meta,
            ";FFMETADATA1\ntitle=Secret title\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=chapter one\n",
        )
        .expect("ffmetadata");
        self.make(
            "tagged.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=640x360:r=10:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
                "-i",
                text(&subtitle),
                "-i",
                text(&meta),
            ],
            &[
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:s",
                "-map_metadata",
                "3",
                "-map_chapters",
                "3",
                "-metadata",
                "location=+50.4501+030.5234/",
                "-metadata:s:v:0",
                "title=Secret stream title",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-c:s",
                "mov_text",
            ],
        )
    }
}

/// Everything ffprobe reports about a file: streams, format and chapters, as JSON.
pub fn report_of(path: &Path) -> serde_json::Value {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-show_chapters",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    assert!(output.status.success(), "ffprobe {}", path.display());
    serde_json::from_slice(&output.stdout).expect("ffprobe json")
}

pub fn streams_of<'a>(report: &'a serde_json::Value, kind: &str) -> Vec<&'a serde_json::Value> {
    report["streams"]
        .as_array()
        .expect("streams")
        .iter()
        .filter(|stream| stream["codec_type"] == kind)
        .collect()
}

/// Every tag key of the format and of each stream, lower-cased.
pub fn tag_keys(report: &serde_json::Value) -> Vec<String> {
    let mut keys = Vec::new();
    let mut collect = |tags: &serde_json::Value| {
        if let Some(map) = tags.as_object() {
            keys.extend(map.keys().map(|key| key.to_lowercase()));
        }
    };
    collect(&report["format"]["tags"]);
    for stream in report["streams"].as_array().expect("streams") {
        collect(&stream["tags"]);
    }
    keys
}

/// True when the `moov` box comes before the first `mdat` among the top-level boxes, read from
/// the file's own bytes.
pub fn moov_before_mdat(path: &Path) -> bool {
    let bytes = std::fs::read(path).expect("read the file");
    let mut at = 0usize;
    while at + 8 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("size")) as usize;
        match &bytes[at + 4..at + 8] {
            b"moov" => return true,
            b"mdat" => return false,
            _ => {}
        }
        let length = match size {
            0 => return false,
            1 => u64::from_be_bytes(bytes[at + 8..at + 16].try_into().expect("size64")) as usize,
            _ => size,
        };
        assert!(length >= 8, "a box shorter than its header at {at}");
        at += length;
    }
    false
}

/// MD5 of the packet payloads of one stream selector (`0:v:0`, `0:a:0`), stream-copied.
pub fn packet_md5(path: &Path, map: &str) -> String {
    let output = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-i",
            text(path),
            "-map",
            map,
            "-c",
            "copy",
            "-f",
            "md5",
            "-",
        ])
        .output()
        .expect("ffmpeg md5");
    assert!(output.status.success(), "md5 of {}", path.display());
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

/// Number of packets of the first video stream.
pub fn video_packets(path: &Path) -> u64 {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_packets",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_packets",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .parse()
        .expect("a packet count")
}
