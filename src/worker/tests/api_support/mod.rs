//! A local in-process HTTP/1.1 listener the client talks to, and the fixtures the API tests share. The peer
//! is a real hyper server (axum); it records every request and answers from a script the test gives it.

#![allow(dead_code)]

use std::fs::{self, File};
use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, Response};
use media_shared::types::{ClaimedJob, JobKind};
use sc_config::Secret;
use sc_ids::{FileHash, FileHasher};
use sunnycloud_worker::{ApiConfig, ScratchDir, ScratchRoot, WorkerApiClient};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub const MARKER: &str = "mK7pQ2xV9nB4tR8wZ3cL6yH1jD5fG0sA2eU4iO7kM9bN3vC5";
pub const WORKER_ID: &str = "worker-under-test";
pub const MIB: u64 = 1 << 20;

pub struct Recorded {
    /// When the request reached the listener.
    pub at: Instant,
    pub method: String,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("content-type", "application/json".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn bytes(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body,
        }
    }
}

pub struct Peer {
    pub addr: SocketAddr,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    task: JoinHandle<()>,
}

impl Peer {
    pub async fn start(script: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&recorded);
        let script = Arc::new(script);
        let app = Router::new().fallback(move |request: Request<Body>| {
            let (log, script) = (Arc::clone(&log), Arc::clone(&script));
            async move {
                let (head, body) = request.into_parts();
                let body = to_bytes(body, usize::MAX).await.expect("request body");
                let seen = Recorded {
                    at: Instant::now(),
                    method: head.method.to_string(),
                    path: head.uri.path().to_owned(),
                    headers: head.headers,
                    body: body.to_vec(),
                };
                let reply = script(&seen);
                log.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(seen);
                let mut response = Response::builder().status(reply.status);
                for (name, value) in reply.headers {
                    response = response.header(name, value);
                }
                response.body(Body::from(reply.body)).expect("response")
            }
        });
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                panic!("test peer stopped: {error}");
            }
        });
        Self {
            addr,
            recorded,
            task,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        let mut guard = self.recorded.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut *guard)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn config(base_url: &str) -> ApiConfig {
    ApiConfig {
        base_url: base_url.to_owned(),
        token: Secret::new(MARKER.to_owned()),
        worker_id: WORKER_ID.to_owned(),
        max_input_photo: 256 * MIB,
        max_input_video: 10 * 1024 * MIB,
    }
}

pub fn client(base_url: &str) -> WorkerApiClient {
    WorkerApiClient::new(config(base_url)).expect("client")
}

/// An address nothing listens on: the port was bound and released.
pub async fn refused_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    drop(listener);
    format!("http://{addr}")
}

/// The pattern the shared hashing vectors use: byte `i` is `i mod 251`.
pub fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// The file hash of `bytes` through the incremental hasher, independent of the client's own roll-up.
pub fn file_hash(bytes: &[u8]) -> FileHash {
    let mut hasher = FileHasher::new();
    for block in bytes.chunks(sc_ids::BLOCK_SIZE) {
        hasher.update(block).expect("block fits");
        hasher.finish_block();
    }
    hasher.finish().1
}

pub fn job(kind: JobKind, content_hash: FileHash) -> ClaimedJob {
    ClaimedJob {
        job_id: Uuid::from_u128(0x5eed),
        kind,
        content_hash,
        params_version: 1,
        attempt: 1,
        lease_ms: 300_000,
    }
}

/// A real scratch tree for one job; the destination of a fetch and the home of an output.
pub struct Scratch {
    _root_dir: TempDir,
    pub dir: ScratchDir,
}

impl Scratch {
    pub fn new(job_id: Uuid) -> Self {
        use std::os::unix::fs::MetadataExt;
        let root_dir = tempfile::tempdir().expect("scratch root");
        let gid = fs::metadata(root_dir.path()).expect("metadata").gid();
        let dir = ScratchRoot::new(root_dir.path(), gid)
            .create(job_id)
            .expect("job scratch");
        Self {
            _root_dir: root_dir,
            dir,
        }
    }

    pub fn source(&self) -> File {
        self.dir.create_source().expect("input file")
    }

    pub fn source_len(&self) -> u64 {
        fs::metadata(self.dir.source_path())
            .expect("input file")
            .len()
    }

    pub fn source_bytes(&self) -> Vec<u8> {
        read_all(&self.dir.source_path())
    }

    pub fn write_output(&self, ext: &str, bytes: &[u8]) {
        fs::write(
            self.dir.path().join("out").join(format!("result.{ext}")),
            bytes,
        )
        .expect("output file");
    }
}

fn read_all(path: &Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    File::open(path)
        .expect("open")
        .read_to_end(&mut bytes)
        .expect("read");
    bytes
}
