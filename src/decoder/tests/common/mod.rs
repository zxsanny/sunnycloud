#![allow(dead_code)]

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use decoder_runner::{
    Ceilings, Exit, Job, JobContext, JobHandler, JobLimits, RunnerConfig, RunnerError, serve,
};
use media_shared::decoder_protocol::{
    Capabilities, FrameReader, ImageResult, Limits, Message, Polled, Reply, Request, Response,
    SelfCheckReport, write_message,
};
use tempfile::TempDir;

pub const MIB: u64 = 1 << 20;

/// A job that runs for a set time, emits evenly spaced progress lines and records what it saw.
pub struct Scripted {
    pub run_for: Duration,
    pub progress_lines: u64,
    pub ignore_cancel: bool,
    pub panics: bool,
    pub jobs: Mutex<Vec<Job>>,
    pub cancels: AtomicUsize,
    pub running: AtomicUsize,
}

impl Scripted {
    pub fn new(run_for: Duration) -> Self {
        Self {
            run_for,
            progress_lines: 0,
            ignore_cancel: false,
            panics: false,
            jobs: Mutex::new(Vec::new()),
            cancels: AtomicUsize::new(0),
            running: AtomicUsize::new(0),
        }
    }

    pub fn lines(mut self, count: u64) -> Self {
        self.progress_lines = count;
        self
    }

    pub fn ignoring_cancel(mut self) -> Self {
        self.ignore_cancel = true;
        self
    }

    pub fn panicking(mut self) -> Self {
        self.panics = true;
        self
    }

    pub fn cancels(&self) -> usize {
        self.cancels.load(Ordering::SeqCst)
    }

    pub fn last_job(&self) -> Job {
        self.jobs
            .lock()
            .expect("jobs")
            .last()
            .expect("a job ran")
            .clone()
    }
}

impl JobHandler for Scripted {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            protocol: 1,
            vips: "v".into(),
            ffmpeg: "f".into(),
            libheif: "h".into(),
            build_timestamp: 1,
            nvenc_probe_ok: false,
            nvenc_sessions: 0,
        }
    }

    fn self_check(&self, _forbidden_targets: &[String]) -> SelfCheckReport {
        SelfCheckReport(Vec::new())
    }

    fn run(&self, job: &Job, context: &JobContext) -> Response {
        assert!(!self.panics, "scripted job panic");
        self.jobs.lock().expect("jobs").push(job.clone());
        self.running.fetch_add(1, Ordering::SeqCst);
        let started = Instant::now();
        let mut emitted = 0;
        let cancelled = loop {
            if !self.ignore_cancel && context.is_cancelled() {
                self.cancels.fetch_add(1, Ordering::SeqCst);
                break true;
            }
            let elapsed = started.elapsed();
            if elapsed >= self.run_for {
                break false;
            }
            let due = (self.progress_lines + 1) * elapsed.as_millis() as u64
                / (self.run_for.as_millis() as u64).max(1);
            while emitted < due.min(self.progress_lines) {
                emitted += 1;
                context.progress(emitted * 1000, 1.5);
            }
            thread::sleep(Duration::from_millis(5));
        };
        self.running.fetch_sub(1, Ordering::SeqCst);
        if cancelled {
            Response::Cancelled
        } else {
            Response::ImageResult(ImageResult {
                width: 1,
                height: 1,
                bytes: 1,
            })
        }
    }
}

pub fn ceilings() -> Ceilings {
    Ceilings {
        photo: JobLimits {
            cpu: 2,
            memory_bytes: 128 * MIB,
            wall: Duration::from_secs(3),
            stall_timeout: Duration::from_secs(30),
            max_output_bytes: 32 * MIB,
        },
        video: JobLimits {
            cpu: 4,
            memory_bytes: 4096 * MIB,
            wall: Duration::from_secs(2400),
            stall_timeout: Duration::from_secs(90),
            max_output_bytes: 10 * 1024 * MIB,
        },
    }
}

pub fn config(path: PathBuf, alive_interval: Duration) -> RunnerConfig {
    RunnerConfig {
        socket_path: path,
        socket_gid: None,
        alive_interval,
        write_timeout: Duration::from_secs(5),
        cancel_wait: Duration::from_secs(4),
        ceilings: ceilings(),
    }
}

pub fn greedy_limits() -> Limits {
    Limits {
        cpu: 64,
        memory_bytes: 64 * 1024 * 1024 * MIB,
        wall: 3_600_000,
        stall_timeout: 3_600_000,
        max_output_bytes: 1 << 40,
    }
}

pub fn render_still() -> Request {
    Request::RenderStill {
        job_dir: "/scratch/job".into(),
        max_edge: 320,
        poster_at_ms: None,
        limits: greedy_limits(),
    }
}

pub fn hello() -> Request {
    Request::Hello { protocol: 1 }
}

pub struct Fixture {
    pub handler: Arc<Scripted>,
    pub path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<Exit, RunnerError>>>,
    _dir: TempDir,
}

impl Fixture {
    pub fn start(handler: Scripted, alive_interval: Duration) -> Self {
        Self::start_with(handler, alive_interval, |_| {})
    }

    pub fn start_with(
        handler: Scripted,
        alive_interval: Duration,
        edit: impl FnOnce(&mut RunnerConfig),
    ) -> Self {
        let handler = Arc::new(handler);
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("decoder.sock");
        let mut config = config(path.clone(), alive_interval);
        edit(&mut config);
        let listener = decoder_runner::socket::bind(&path, None).expect("bind");
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (handler, stop) = (Arc::clone(&handler), Arc::clone(&stop));
            thread::spawn(move || serve_on(&listener, &handler, &config, &stop))
        };
        Self {
            handler,
            path,
            stop,
            thread: Some(thread),
            _dir: dir,
        }
    }

    pub fn connect(&self) -> Client {
        Client::new(UnixStream::connect(&self.path).expect("connect"))
    }

    /// Waits for `serve` to return by itself (violation or fatal error).
    pub fn outcome(mut self) -> Result<Exit, RunnerError> {
        let thread = self.thread.take().expect("serve thread");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !thread.is_finished() {
            assert!(Instant::now() < deadline, "serve did not return in 5 s");
            thread::sleep(Duration::from_millis(10));
        }
        thread.join().expect("serve thread panicked")
    }
}

fn serve_on(
    listener: &UnixListener,
    handler: &Arc<Scripted>,
    config: &RunnerConfig,
    stop: &AtomicBool,
) -> Result<Exit, RunnerError> {
    serve(listener, handler, config, stop)
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

// A short-lived value matched at once in the tests: boxing would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Received {
    Reply(Reply),
    Silence,
    Closed,
}

pub struct Client {
    pub stream: UnixStream,
    reader: FrameReader,
}

impl Client {
    pub fn new(stream: UnixStream) -> Self {
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("read timeout");
        Self {
            stream,
            reader: FrameReader::new(),
        }
    }

    pub fn send(&mut self, request: Request) {
        write_message(&mut self.stream, &Message::Request(request)).expect("send request");
    }

    pub fn send_raw(&mut self, bytes: &[u8]) {
        use std::io::Write;
        self.stream.write_all(bytes).expect("send raw");
    }

    pub fn send_payload(&mut self, payload: &[u8]) {
        let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(payload);
        self.send_raw(&frame);
    }

    /// The next reply, or why there is none within `within`.
    pub fn recv(&mut self, within: Duration) -> Received {
        let deadline = Instant::now() + within;
        loop {
            match self.reader.poll_reply(&mut self.stream) {
                Ok(Polled::Ready(reply)) => return Received::Reply(reply),
                Ok(Polled::Closed) => return Received::Closed,
                Ok(Polled::Pending) => {}
                Err(decoder_error) => {
                    // A reset after unread data counts as the runner closing the connection.
                    if matches!(
                        &decoder_error,
                        media_shared::decoder_protocol::ReadError::Io(_)
                    ) {
                        return Received::Closed;
                    }
                    panic!("runner sent an invalid frame: {decoder_error}");
                }
            }
            if Instant::now() >= deadline {
                return Received::Silence;
            }
        }
    }

    pub fn expect_response(&mut self, within: Duration) -> Response {
        match self.recv(within) {
            Received::Reply(Reply::Response(response)) => response,
            other => panic!("expected a response, got {other:?}"),
        }
    }
}
