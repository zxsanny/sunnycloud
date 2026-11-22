//! Media made at test time by the host's own `vips` and `ffmpeg`: noise pictures in each format,
//! the clips, and the header-only PNG of a decompression bomb.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use super::media::run_tool;

/// A directory of generated media: noise images in each format, and the two clips.
pub struct Media {
    dir: TempDir,
}

impl Media {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("media dir"),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn text(&self, name: &str) -> String {
        self.path(name).to_str().expect("utf8").to_owned()
    }

    /// A noise picture of `width` x `height` saved by libvips as the format of `name`'s extension.
    pub fn image(&self, name: &str, width: u32, height: u32) -> Vec<u8> {
        let raw = self.text(&format!("{name}.v"));
        run_tool(
            "vips",
            &["gaussnoise", &raw, &width.to_string(), &height.to_string()],
        );
        let out = self.text(name);
        let save = match name.rsplit('.').next().expect("extension") {
            "jpg" => "jpegsave",
            "png" => "pngsave",
            "webp" => "webpsave",
            "heic" => "heifsave",
            "gif" => "gifsave",
            other => panic!("no saver for {other}"),
        };
        run_tool("vips", &[save, &raw, &out]);
        std::fs::read(self.path(name)).expect("read generated image")
    }

    /// A JPEG whose EXIF orientation is `orientation`, made by patching the tag libvips writes.
    pub fn oriented_jpeg(&self, width: u32, height: u32, orientation: u8) -> Vec<u8> {
        let mut bytes = self.image("oriented.jpg", width, height);
        let at = set_orientation_at(&bytes);
        bytes[at] = orientation;
        bytes
    }

    fn ffmpeg(&self, input: &[&str], output: &str, rest: &[&str]) -> Vec<u8> {
        let out = self.text(output);
        let mut args = vec!["-loglevel", "error", "-y"];
        args.extend_from_slice(input);
        args.extend_from_slice(rest);
        args.push(&out);
        run_tool("ffmpeg", &args);
        std::fs::read(self.path(output)).expect("read generated clip")
    }

    /// A `seconds` long H.264 MP4 of the test pattern; the index (`moov`) is at the end.
    pub fn mp4(&self, seconds: u32) -> Vec<u8> {
        let source = format!("testsrc=d={seconds}:s=320x240:r=10");
        self.ffmpeg(
            &["-f", "lavfi", "-i", &source],
            "clip.mp4",
            &["-pix_fmt", "yuv420p"],
        )
    }

    /// The same clip with the index first.
    pub fn faststart_mp4(&self, seconds: u32) -> Vec<u8> {
        let source = format!("testsrc=d={seconds}:s=320x240:r=10");
        self.ffmpeg(
            &["-f", "lavfi", "-i", &source],
            "fast.mp4",
            &["-pix_fmt", "yuv420p", "-movflags", "+faststart"],
        )
    }

    /// A 3 s H.264 plus AAC MP4 (a phone clip); the index is first when `faststart`.
    pub fn av_mp4(&self, faststart: bool) -> Vec<u8> {
        let rest: &[&str] = if faststart {
            &["-pix_fmt", "yuv420p", "-movflags", "+faststart"]
        } else {
            &["-pix_fmt", "yuv420p"]
        };
        self.ffmpeg(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=d=3:s=320x240:r=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
            ],
            "av.mp4",
            rest,
        )
    }

    /// A QuickTime file (brand `qt  `).
    pub fn mov(&self) -> Vec<u8> {
        self.ffmpeg(
            &["-f", "lavfi", "-i", "testsrc=d=1:s=320x240:r=10"],
            "clip.mov",
            &["-pix_fmt", "yuv420p", "-f", "mov"],
        )
    }

    /// Three seconds at 10 fps whose frame `n` is a flat grey of luma `8 * n`, so the frame a
    /// poster shows is told by its brightness.
    pub fn stepped_clip(&self) -> Vec<u8> {
        let source = "color=c=black:s=64x64:r=10:d=3,format=yuv420p,geq=lum='8*N':cb=128:cr=128";
        self.ffmpeg(
            &["-f", "lavfi", "-i", source],
            "stepped.mp4",
            &["-pix_fmt", "yuv420p", "-movflags", "+faststart"],
        )
    }

    /// The clip with a display-matrix rotation of 90 degrees (a phone held upright).
    pub fn rotated_mp4(&self) -> Vec<u8> {
        std::fs::write(self.path("unrotated.mp4"), self.mp4(2)).expect("write clip");
        let input = self.text("unrotated.mp4");
        self.ffmpeg(
            &["-display_rotation", "90", "-i", &input],
            "rotated.mp4",
            &["-c", "copy"],
        )
    }

    /// A 10-bit HEVC clip tagged HLG, BT.2020.
    pub fn hlg_mp4(&self) -> Vec<u8> {
        self.ffmpeg(
            &["-f", "lavfi", "-i", "testsrc=d=2:s=320x240:r=10"],
            "hlg.mp4",
            &[
                "-vf",
                "format=yuv420p10le",
                "-c:v",
                "libx265",
                "-x265-params",
                "colorprim=bt2020:transfer=arib-std-b67:colormatrix=bt2020nc:log-level=error",
            ],
        )
    }
}

/// The mean of all pixel values of an image file, as printed by `vips avg`.
pub fn mean_of(path: &Path) -> f64 {
    let output = Command::new("vips")
        .arg("avg")
        .arg(path)
        .output()
        .expect("vips avg");
    assert!(output.status.success(), "{}", path.display());
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .parse()
        .expect("a number")
}

/// What `ffprobe` reports about the first video stream, read independently of the runner.
pub fn ffprobe_fields(path: &Path, entries: &str) -> String {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            entries,
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    assert!(output.status.success(), "{}", path.display());
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

/// Where the EXIF orientation value is in a JPEG written by libvips (little-endian TIFF).
pub fn set_orientation_at(jpeg: &[u8]) -> usize {
    let tag = [0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00];
    jpeg.windows(tag.len())
        .position(|window| window == tag)
        .expect("libvips writes an orientation tag")
        + tag.len()
}

/// A PNG whose header claims `width` x `height` and whose pixel data is absent: only a header
/// read can accept it, and no decoder can finish it.
pub fn png_header_only(width: u32, height: u32) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"IDAT", &[0x78, 0x9c, 0x01]));
    png
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    chunk.extend_from_slice(&body);
    chunk.extend_from_slice(&crc32(&body).to_be_bytes());
    chunk
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
