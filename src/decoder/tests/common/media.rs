//! Real-tool test support: the real `decoder-runner` binary on a real socket, and media files
//! made at test time by the host's own `vips` and `ffmpeg`.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{ImageResult, Limits, Request, Response};
use tempfile::TempDir;

use super::{Client, MIB};

pub const WAIT: Duration = Duration::from_secs(20);

pub struct MediaRunner {
    child: Child,
    env: Vec<(String, String)>,
    path: String,
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
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        let child = spawn_runner(&socket, &scratch, path, &env);
        let runner = Self {
            child,
            env,
            path: path.to_owned(),
            socket,
            scratch,
            next_job: 0,
            _dir: dir,
        };
        runner.wait_for_socket();
        runner
    }

    /// Starts a new runner process in place of the one that ended (a hostile-class outcome ends
    /// the process and the container restarts it): same scratch tree, same settings, tools found
    /// in `path` (a restarted container has the real tools again).
    pub fn restart_with_path(&mut self, path: &str) {
        self.path = path.to_owned();
        assert_eq!(
            self.end_within(WAIT),
            RunnerEnd::Exited(3),
            "the runner must have ended before it is restarted"
        );
        self.child = spawn_runner(&self.socket, &self.scratch, &self.path, &self.env);
        self.wait_for_socket();
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

    /// Everything the runner has logged so far.
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.socket.with_file_name(LOG_FILE)).expect("read the runner log")
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
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

fn spawn_runner(socket: &Path, scratch: &Path, path: &str, env: &[(String, String)]) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_decoder-runner"));
    command
        .env_clear()
        .env("PATH", path)
        .env("SUNNYCLOUD_ENVIRONMENT", "development")
        .env("SUNNYCLOUD_DECODER_SOCKET_PATH", socket)
        .env("SUNNYCLOUD_DECODER_SCRATCH_ROOT", scratch);
    for (key, value) in env {
        command.env(key, value);
    }
    // The runner's log is kept for the cases that look for what it must not have written.
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(socket.with_file_name(LOG_FILE))
        .expect("open the runner log");
    command
        .stdout(log.try_clone().expect("clone the log handle"))
        .stderr(log)
        .spawn()
        .expect("spawn runner")
}

const LOG_FILE: &str = "runner.log";

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
    path_with_scripts(&[("vips", stub)])
}

/// A directory that is the runner's whole `PATH`: the real tools, except that each tool named
/// in `scripts` is a shell script with that body.
pub fn path_with_scripts(scripts: &[(&str, &str)]) -> (TempDir, String) {
    let dir = TempDir::new().expect("bin dir");
    for tool in ["vips", "vipsheader", "ffmpeg", "ffprobe"] {
        let target = dir.path().join(tool);
        match scripts.iter().find(|(name, _)| *name == tool) {
            Some((_, body)) => {
                fs::write(&target, format!("#!/bin/sh\n{body}\n")).expect("write stub");
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                    .expect("chmod stub");
            }
            None => symlink(real_tool(tool), &target).expect("link real tool"),
        }
    }
    let path = dir.path().to_str().expect("utf8").to_owned();
    (dir, path)
}

pub fn real_tool(tool: &str) -> std::path::PathBuf {
    std::env::var("PATH")
        .expect("PATH")
        .split(':')
        .map(|dir| Path::new(dir).join(tool))
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| panic!("{tool} is not installed"))
}

/// Every live process whose chain of parents reaches `root`, read from the process table.
pub fn live_descendants(root: u32) -> Vec<u32> {
    let table = decoder_runner::proc_table::snapshot().expect("process table");
    descendants_in(&table, root)
        .into_iter()
        .map(|process| process.pid)
        .collect()
}

fn descendants_in(
    table: &[decoder_runner::proc_table::Process],
    root: u32,
) -> Vec<decoder_runner::proc_table::Process> {
    let mut found: Vec<decoder_runner::proc_table::Process> = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for process in table.iter().filter(|process| process.ppid == parent) {
            if !found.iter().any(|known| known.pid == process.pid) {
                found.push(*process);
                frontier.push(process.pid);
            }
        }
    }
    found.retain(|process| !process.zombie);
    found
}

/// The resident memory of the runner and of everything it started, summed from one process-table
/// snapshot.
pub fn group_rss(root: u32) -> u64 {
    let table = decoder_runner::proc_table::snapshot().expect("process table");
    let own = table
        .iter()
        .find(|process| process.pid == root)
        .map_or(0, |process| process.rss_bytes);
    own + descendants_in(&table, root)
        .iter()
        .map(|process| process.rss_bytes)
        .sum::<u64>()
}

/// Samples [`group_rss`] on a thread until [`PeakWatch::peak`] is called.
pub struct PeakWatch {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: thread::JoinHandle<u64>,
}

impl PeakWatch {
    pub fn start(root: u32) -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut peak = group_rss(root);
            while !flag.load(std::sync::atomic::Ordering::Acquire) {
                peak = peak.max(group_rss(root));
                thread::sleep(Duration::from_millis(2));
            }
            peak
        });
        Self { stop, thread }
    }

    pub fn peak(self) -> u64 {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.thread.join().expect("the sampler thread")
    }
}

/// Seconds since boot from `/proc/uptime` (10 ms resolution): a stamp a shell stub can write.
pub fn uptime_seconds() -> f64 {
    fs::read_to_string("/proc/uptime")
        .expect("read /proc/uptime")
        .split_whitespace()
        .next()
        .and_then(|text| text.parse().ok())
        .expect("uptime in seconds")
}

/// How long ago `stamp` (a [`uptime_seconds`] value) was.
pub fn age_of(stamp: f64) -> Duration {
    Duration::from_secs_f64((uptime_seconds() - stamp).max(0.0))
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

// Each test binary uses some of these.
#[allow(unused_imports)]
pub use super::generated::{Media, ffprobe_fields, mean_of, png_header_only, set_orientation_at};
