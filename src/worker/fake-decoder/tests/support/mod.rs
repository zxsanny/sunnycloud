//! A supervisor-side client of FD for the tests, and the folders a job request points at.

#![allow(dead_code)]

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use media_shared::decoder_protocol::{
    Limits, MAX_PAYLOAD, Message, Reply, Request, parse_reply, write_message,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// The longest any single step of a test waits before it fails.
pub const WAIT: Duration = Duration::from_secs(10);

pub struct Client {
    stream: UnixStream,
}

impl Client {
    pub async fn connect(socket: &Path) -> Self {
        let stream = UnixStream::connect(socket).await.expect("FD accepts");
        Self { stream }
    }

    pub async fn send(&mut self, request: Request) {
        let mut frame = Vec::new();
        write_message(&mut frame, &Message::Request(request)).expect("a valid request");
        self.stream
            .write_all(&frame)
            .await
            .expect("the request is sent");
    }

    /// The next reply, or `None` when FD closed the connection.
    pub async fn reply(&mut self) -> Option<Reply> {
        tokio::time::timeout(WAIT, self.read_reply())
            .await
            .expect("FD answers in time")
    }

    async fn read_reply(&mut self) -> Option<Reply> {
        let mut prefix = [0u8; 4];
        match self.stream.read_exact(&mut prefix).await {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return None,
            Err(error) => panic!("reading a reply failed: {error}"),
        }
        let length = u32::from_be_bytes(prefix) as usize;
        assert!(length <= MAX_PAYLOAD, "FD announced {length} bytes");
        let mut payload = vec![0u8; length];
        self.stream
            .read_exact(&mut payload)
            .await
            .expect("the whole frame");
        Some(parse_reply(&payload).expect("FD sends valid replies"))
    }

    /// Exactly `count` raw bytes.
    pub async fn raw(&mut self, count: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; count];
        tokio::time::timeout(WAIT, self.stream.read_exact(&mut bytes))
            .await
            .expect("FD sends in time")
            .expect("the bytes arrive");
        bytes
    }

    /// True when nothing has arrived yet and the connection is still open.
    pub fn nothing_pending(&self) -> bool {
        let mut byte = [0u8; 1];
        matches!(self.stream.try_read(&mut byte), Err(error) if error.kind() == ErrorKind::WouldBlock)
    }
}

/// A job tree as the supervisor makes it: `<root>/<job id>/in/source` and an empty `out/`.
pub struct JobDir {
    _root: TempDir,
    pub path: PathBuf,
}

impl JobDir {
    pub fn new(job_id: &str) -> Self {
        let root = tempfile::tempdir().expect("scratch root");
        let path = root.path().join(job_id);
        fs::create_dir_all(path.join("in")).expect("in/");
        fs::create_dir_all(path.join("out")).expect("out/");
        fs::write(path.join("in/source"), b"source bytes").expect("in/source");
        Self { _root: root, path }
    }

    pub fn dir(&self) -> String {
        self.path.display().to_string()
    }

    pub fn out(&self, name: &str) -> PathBuf {
        self.path.join("out").join(name)
    }
}

pub fn limits() -> Limits {
    Limits {
        cpu: 2,
        memory_bytes: 1 << 30,
        wall: 120_000,
        stall_timeout: 90_000,
        max_output_bytes: 32 << 20,
    }
}

pub fn hello() -> Request {
    Request::Hello { protocol: 2 }
}

pub fn self_check() -> Request {
    Request::SelfCheck {
        forbidden_targets: vec!["db:5432".into()],
    }
}

pub fn probe(job: &JobDir) -> Request {
    Request::Probe { job_dir: job.dir() }
}

pub fn render(job: &JobDir, max_edge: u32) -> Request {
    Request::RenderStill {
        job_dir: job.dir(),
        max_edge,
        poster_at_ms: None,
        params_version: 1,
        limits: limits(),
    }
}

/// Polls `condition` every few milliseconds until it holds; fails the test after [`WAIT`].
pub async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let give_up = tokio::time::Instant::now() + WAIT;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < give_up,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// The lines of a record file, each parsed.
pub fn record_lines(path: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).expect("each record line is JSON"))
        .collect()
}

pub fn kinds(lines: &[serde_json::Value]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line["kind"].as_str().expect("a kind").to_owned())
        .collect()
}
