//! Real-tool test support: the real `decoder-runner` binary on a real socket, and media files
//! made at test time by the host's own `vips` and `ffmpeg`.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{ImageResult, Limits, Request, Response};
use tempfile::TempDir;

use super::{Client, MIB};

pub const WAIT: Duration = Duration::from_secs(20);

pub struct MediaRunner {
    child: Child,
    socket: PathBuf,
    pub scratch: PathBuf,
    next_job: u32,
    _dir: TempDir,
}

impl MediaRunner {
    pub fn start() -> Self {
        Self::start_with(&[], &std::env::var("PATH").expect("PATH"))
    }

    /// `env` adds settings; `path` is the `PATH` the runner finds its tools in.
    pub fn start_with(env: &[(&str, &str)], path: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let socket = dir.path().join("decoder.sock");
        let scratch = dir.path().join("scratch");
        std::fs::create_dir_all(&scratch).expect("scratch root");
        let mut command = Command::new(env!("CARGO_BIN_EXE_decoder-runner"));
        command
            .env_clear()
            .env("PATH", path)
            .env("SUNNYCLOUD_ENVIRONMENT", "development")
            .env("SUNNYCLOUD_DECODER_SOCKET_PATH", &socket)
            .env("SUNNYCLOUD_DECODER_SCRATCH_ROOT", &scratch);
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn runner");
        let runner = Self {
            child,
            socket,
            scratch,
            next_job: 0,
            _dir: dir,
        };
        runner.wait_for_socket();
        runner
    }

    fn wait_for_socket(&self) {
        let deadline = Instant::now() + WAIT;
        while UnixStream::connect(&self.socket).is_err() {
            assert!(Instant::now() < deadline, "runner never listened");
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn connect(&self) -> Client {
        Client::new(UnixStream::connect(&self.socket).expect("connect"))
    }

    /// A job directory holding `content` as `in/source` and an empty `out/`.
    pub fn job(&mut self, content: &[u8]) -> PathBuf {
        self.next_job += 1;
        let job = self.scratch.join(format!("job{}", self.next_job));
        std::fs::create_dir_all(job.join("in")).expect("in/");
        std::fs::create_dir_all(job.join("out")).expect("out/");
        std::fs::write(job.join("in/source"), content).expect("in/source");
        job
    }

    /// How the runner ended, waiting at most `within` for it to end.
    pub fn end_within(&mut self, within: Duration) -> RunnerEnd {
        let deadline = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return match (status.code(), status.signal()) {
                    (Some(code), _) => RunnerEnd::Exited(code),
                    (None, signal) => RunnerEnd::Signaled(signal),
                };
            }
            if Instant::now() >= deadline {
                return RunnerEnd::StillRunning;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// How a runner process ended; a signal and "did not end" are different failures.
#[derive(Debug, PartialEq, Eq)]
pub enum RunnerEnd {
    Exited(i32),
    Signaled(Option<i32>),
    StillRunning,
}

impl Drop for MediaRunner {
    fn drop(&mut self) {
        if self.child.kill().is_ok() {
            self.child.wait().expect("reap runner");
        }
    }
}

/// A directory that is the runner's whole `PATH`: the real `vipsheader`, `ffmpeg`, `ffprobe`
/// and a `vips` (the thumbnail step) running the given script.
pub fn path_with_stub(stub: &str) -> (TempDir, String) {
    let dir = TempDir::new().expect("bin dir");
    for tool in ["vipsheader", "ffmpeg", "ffprobe"] {
        symlink(real(tool), dir.path().join(tool)).expect("link real tool");
    }
    let script = dir.path().join("vips");
    fs::write(&script, format!("#!/bin/sh\n{stub}\n")).expect("write stub");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod stub");
    let path = dir.path().to_str().expect("utf8").to_owned();
    (dir, path)
}

fn real(tool: &str) -> std::path::PathBuf {
    std::env::var("PATH")
        .expect("PATH")
        .split(':')
        .map(|dir| Path::new(dir).join(tool))
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| panic!("{tool} is not installed"))
}

pub fn alive(marker: &str) -> bool {
    fs::read_dir("/proc")
        .expect("proc")
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read(entry.path().join("cmdline")).ok())
        .any(|cmdline| String::from_utf8_lossy(&cmdline).contains(marker))
}

/// A killed process leaves the table a moment after the kill is delivered.
pub fn wait_until_gone(markers: &[&str]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while markers.iter().any(|marker| alive(marker)) {
        assert!(Instant::now() < deadline, "{markers:?} still alive");
        std::thread::yield_now();
    }
}

pub fn limits() -> Limits {
    Limits {
        cpu: 2,
        memory_bytes: 2048 * MIB,
        wall: 60_000,
        stall_timeout: 60_000,
        max_output_bytes: 32 * MIB,
    }
}

pub fn render(client: &mut Client, job: &Path, max_edge: u32, limits: Limits) -> Response {
    client.send(Request::RenderStill {
        job_dir: job.to_str().expect("utf8").into(),
        max_edge,
        poster_at_ms: None,
        limits,
    });
    client.expect_response(WAIT)
}

pub fn render_at(client: &mut Client, job: &Path, max_edge: u32, poster_at_ms: u64) -> Response {
    client.send(Request::RenderStill {
        job_dir: job.to_str().expect("utf8").into(),
        max_edge,
        poster_at_ms: Some(poster_at_ms),
        limits: limits(),
    });
    client.expect_response(WAIT)
}

pub fn probe(client: &mut Client, job: &Path) -> Response {
    client.send(Request::Probe {
        job_dir: job.to_str().expect("utf8").into(),
    });
    client.expect_response(WAIT)
}

pub fn image_result(response: Response) -> ImageResult {
    match response {
        Response::ImageResult(result) => result,
        other => panic!("expected an image result, got {other:?}"),
    }
}

/// Runs a host tool and fails the test when it does not succeed.
pub fn run_tool(program: &str, args: &[&str]) {
    let output = Command::new(program).args(args).output().expect(program);
    assert!(
        output.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `width` x `height` of what `vipsheader` reads from `path`, one field per call (libvips 8.15
/// prints only the last of several `-f` options).
pub fn header_of(path: &Path) -> (u32, u32) {
    let field = |name: &str| {
        let output = Command::new("vipsheader")
            .args(["-f", name])
            .arg(path)
            .output()
            .expect("vipsheader");
        assert!(output.status.success(), "{}", path.display());
        String::from_utf8(output.stdout)
            .expect("utf8")
            .trim()
            .parse::<u32>()
            .expect("number")
    };
    (field("width"), field("height"))
}

/// A 96x64 HEIC with an `irot` transform (rotation by 270 degrees): 64x96 once decoded. Made once
/// on libvips 8.18 and kept as hex text (no binary in git) because libvips 8.15 `heifsave` writes no
/// `irot`, so the file cannot be generated the same way on every host.
pub fn rotated_heic() -> Vec<u8> {
    let hex = include_str!("../fixtures/rotated-96x64-irot270.heic.hex").trim();
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("the fixture is hex"))
        .collect()
}

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
fn set_orientation_at(jpeg: &[u8]) -> usize {
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
