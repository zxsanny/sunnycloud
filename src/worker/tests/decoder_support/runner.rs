use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use decoder_runner::{
    Ceilings, Exit, Heartbeat, Job, JobContext, JobHandler, JobLimits, RunnerConfig, RunnerError,
    serve,
};
use media_shared::decoder_protocol::{Capabilities, Response, SelfCheckItem, SelfCheckReport};
use tempfile::TempDir;

const MIB: u64 = 1 << 20;
pub const BUILD_TIMESTAMP: u64 = 1_700_000_000;

pub fn capabilities() -> Capabilities {
    Capabilities {
        protocol: 1,
        vips: "8.15".into(),
        ffmpeg: "7.1".into(),
        libheif: "1.19".into(),
        build_timestamp: BUILD_TIMESTAMP,
        nvenc_probe_ok: true,
        nvenc_sessions: 3,
    }
}

pub fn ok_items() -> Vec<SelfCheckItem> {
    ["no_network", "no_secrets", "no_blob_mount"]
        .into_iter()
        .map(|name| SelfCheckItem {
            name: name.into(),
            ok: true,
        })
        .collect()
}

/// What the runner records and answers. `run` answers with `answer` after `run_for`, or `Cancelled` when
/// the client cancels first.
pub struct Handler {
    pub requests: Mutex<Vec<&'static str>>,
    pub targets: Mutex<Vec<Vec<String>>>,
    pub items: Mutex<Vec<SelfCheckItem>>,
    pub answer: Mutex<Response>,
    pub run_for: Mutex<Duration>,
    pub progress_lines: AtomicUsize,
    pub cancels: AtomicUsize,
}

impl Handler {
    pub fn new(answer: Response) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            targets: Mutex::new(Vec::new()),
            items: Mutex::new(ok_items()),
            answer: Mutex::new(answer),
            run_for: Mutex::new(Duration::ZERO),
            progress_lines: AtomicUsize::new(0),
            cancels: AtomicUsize::new(0),
        })
    }

    pub fn log(&self) -> Vec<&'static str> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, request: &'static str) {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request);
    }
}

impl JobHandler for Handler {
    fn capabilities(&self) -> Capabilities {
        self.record("hello");
        capabilities()
    }

    fn self_check(&self, forbidden_targets: &[String]) -> SelfCheckReport {
        self.record("self_check");
        self.targets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(forbidden_targets.to_vec());
        SelfCheckReport(
            self.items
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        )
    }

    fn run(&self, _job: &Job, context: &JobContext) -> Response {
        self.record("job");
        let run_for = *self.run_for.lock().unwrap_or_else(PoisonError::into_inner);
        let started = Instant::now();
        for line in 1..=self.progress_lines.load(Ordering::SeqCst) {
            context.progress(line as u64 * 1000, 1.5);
        }
        while started.elapsed() < run_for {
            if context.is_cancelled() {
                self.cancels.fetch_add(1, Ordering::SeqCst);
                return Response::Cancelled;
            }
            thread::sleep(Duration::from_millis(5));
        }
        self.answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn config(socket: PathBuf, scratch_root: PathBuf, alive_interval: Duration) -> RunnerConfig {
    let limits = |wall| JobLimits {
        cpu: 2,
        memory_bytes: 128 * MIB,
        wall,
        stall_timeout: Duration::from_secs(30),
        max_output_bytes: 32 * MIB,
    };
    RunnerConfig {
        socket_path: socket,
        socket_gid: None,
        scratch_root,
        alive_interval,
        watchdog: Duration::from_secs(30),
        write_timeout: Duration::from_secs(5),
        cancel_wait: Duration::from_secs(4),
        ceilings: Ceilings {
            photo: limits(Duration::from_secs(30)),
            video: limits(Duration::from_secs(60)),
        },
    }
}

/// The real runner socket loop on its own thread, listening at `socket`. The runner checks every job
/// directory before the handler runs, so it owns a real scratch tree with one job holding a regular
/// `in/source` file.
pub struct Runner {
    scratch: TempDir,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<Exit, RunnerError>>>,
}

impl Runner {
    pub fn start(socket: &Path, handler: &Arc<Handler>, alive_interval: Duration) -> Self {
        let listener: UnixListener =
            decoder_runner::socket::bind(socket, None).expect("the runner binds its socket");
        let scratch = tempfile::tempdir().expect("a scratch root");
        let input = scratch.path().join("job").join("in");
        std::fs::create_dir_all(&input).expect("the job input folder");
        std::fs::write(input.join("source"), b"source bytes").expect("the job input file");
        let config = config(
            socket.to_path_buf(),
            scratch.path().to_path_buf(),
            alive_interval,
        );
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (handler, stop) = (Arc::clone(handler), Arc::clone(&stop));
            thread::spawn(move || serve(&listener, &handler, &config, &stop, &Heartbeat::new()))
        };
        Self {
            scratch,
            stop,
            thread: Some(thread),
        }
    }

    /// The job directory to send in a request: inside the runner's scratch root.
    pub fn job_dir(&self) -> String {
        self.scratch.path().join("job").display().to_string()
    }

    /// The runner exits: the loop ends and the listener closes, leaving the stale socket file as a crash
    /// would. Every client connection must be closed first, because the loop serves one at a time.
    pub fn exit(mut self) -> Result<Exit, RunnerError> {
        self.stop.store(true, Ordering::SeqCst);
        self.thread
            .take()
            .expect("the runner thread")
            .join()
            .expect("the runner thread did not panic")
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
