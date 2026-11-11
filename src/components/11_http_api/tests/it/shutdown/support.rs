use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use contracts::{
    AcceptOutcome, ByteStream, ClaimedJob, DerivedKind, FailDisposition, FailResult, Lane,
    LeaseExtended, OutputMeta, SkipOutcome, StreamError, WorkerInput, WorkerJobError, WorkerJobs,
    WorkerStatus,
};
use futures_core::Stream;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use sunnycloud_http_api::{ApiServices, HttpConfig, HttpServer, HttpServerError};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Instant, Sleep};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::auth::support::{ScriptedWorkerAuth, WORKER_SECRET};
use crate::harness::script::unscripted;
use crate::harness::{EVENT_BOUND, Script, default_config, new_metrics, stub_services, wait_until};

/// Short enough for a test, long enough to hold the two downloads of AC-2 apart (1 s and 5 s).
pub const GRACE: Duration = Duration::from_secs(2);
const PACE: Duration = Duration::from_millis(100);
const CHUNK: usize = 1024;

/// A download that needs 1 s: it ends inside the grace.
pub const SHORT_JOB: Uuid = Uuid::from_u128(0xD1);
/// A download that needs 5 s: it is still running when the grace ends.
pub const LONG_JOB: Uuid = Uuid::from_u128(0xD2);
const SHORT_CHUNKS: usize = 10;
const LONG_CHUNKS: usize = 50;
const WORKER: &str = "worker-7";

pub fn short_body_len() -> usize {
    SHORT_CHUNKS * CHUNK
}

/// `WorkerJobs` for the shutdown tests. `claim` is held until `ApiServices::feed_shutdown` is cancelled, then
/// reports itself woken and answers "no job" once the test releases it; `open_input` streams one chunk per
/// [`PACE`].
pub struct HeldJobs {
    feed_shutdown: CancellationToken,
    release: CancellationToken,
    held: AtomicUsize,
    woken: AtomicUsize,
    downloads: AtomicUsize,
}

impl HeldJobs {
    fn new(feed_shutdown: CancellationToken) -> Arc<Self> {
        Arc::new(HeldJobs {
            feed_shutdown,
            release: CancellationToken::new(),
            held: AtomicUsize::new(0),
            woken: AtomicUsize::new(0),
            downloads: AtomicUsize::new(0),
        })
    }

    pub async fn wait_held(&self, count: usize) {
        wait_until("claims to be held", || async {
            self.held.load(Ordering::Acquire) == count
        })
        .await;
    }

    pub async fn wait_woken(&self, count: usize) {
        wait_until("held claims to be woken", || async {
            self.woken.load(Ordering::Acquire) == count
        })
        .await;
    }

    pub async fn wait_downloads(&self, count: usize) {
        wait_until("downloads to start", || async {
            self.downloads.load(Ordering::Acquire) == count
        })
        .await;
    }

    /// Lets the woken claims answer.
    pub fn release_claims(&self) {
        self.release.cancel();
    }
}

/// `chunks` chunks of [`CHUNK`] bytes, one per [`PACE`].
struct Paced {
    left: usize,
    timer: Pin<Box<Sleep>>,
}

impl Stream for Paced {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.left == 0 {
            return Poll::Ready(None);
        }
        if self.timer.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        self.left -= 1;
        self.timer.as_mut().reset(Instant::now() + PACE);
        Poll::Ready(Some(Ok(Bytes::from(vec![7u8; CHUNK]))))
    }
}

#[contracts::async_trait]
impl WorkerJobs for HeldJobs {
    async fn claim(&self, _: Lane, _: &str) -> Result<Option<ClaimedJob>, WorkerJobError> {
        self.held.fetch_add(1, Ordering::AcqRel);
        self.feed_shutdown.cancelled().await;
        self.woken.fetch_add(1, Ordering::AcqRel);
        self.release.cancelled().await;
        Ok(None)
    }

    async fn open_input(&self, job_id: Uuid, _: &str) -> Result<WorkerInput, WorkerJobError> {
        let chunks = if job_id == LONG_JOB {
            LONG_CHUNKS
        } else {
            SHORT_CHUNKS
        };
        self.downloads.fetch_add(1, Ordering::AcqRel);
        Ok(WorkerInput {
            length: u64::try_from(chunks * CHUNK).unwrap_or_default(),
            body: Box::pin(Paced {
                left: chunks,
                timer: Box::pin(tokio::time::sleep(PACE)),
            }),
        })
    }

    async fn extend_lease(&self, _: Uuid, _: &str) -> Result<LeaseExtended, WorkerJobError> {
        Err(WorkerJobError::Unexpected(unscripted("extend_lease")))
    }

    async fn release(&self, _: Uuid, _: &str) -> Result<(), WorkerJobError> {
        Err(WorkerJobError::Unexpected(unscripted("release")))
    }

    async fn complete_skipped(
        &self,
        _: Uuid,
        _: &str,
        _: SkipOutcome,
    ) -> Result<(), WorkerJobError> {
        Err(WorkerJobError::Unexpected(unscripted("complete_skipped")))
    }

    async fn fail(
        &self,
        _: Uuid,
        _: &str,
        _: &str,
        _: FailDisposition,
    ) -> Result<FailResult, WorkerJobError> {
        Err(WorkerJobError::Unexpected(unscripted("fail")))
    }

    async fn accept_output(
        &self,
        _: Uuid,
        _: &str,
        _: DerivedKind,
        _: OutputMeta,
        _: ByteStream,
        _: u64,
    ) -> Result<AcceptOutcome, WorkerJobError> {
        Err(WorkerJobError::Unexpected(unscripted("accept_output")))
    }

    async fn record_status(&self, _: &str, _: WorkerStatus) -> Result<(), WorkerJobError> {
        Err(WorkerJobError::Unexpected(unscripted("record_status")))
    }
}

/// `run` over the stubs, with [`GRACE`] as the shutdown grace.
pub struct Rig {
    pub public: SocketAddr,
    pub internal: SocketAddr,
    pub ops: SocketAddr,
    pub jobs: Arc<HeldJobs>,
    signal: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), HttpServerError>>,
}

impl Rig {
    pub async fn start() -> Rig {
        let feed_shutdown = CancellationToken::new();
        let jobs = HeldJobs::new(feed_shutdown.clone());
        let services = ApiServices {
            feed_shutdown,
            worker_auth: Arc::new(ScriptedWorkerAuth::default()),
            worker_jobs: jobs.clone(),
            ..stub_services(&Script::default(), new_metrics())
        };
        let config = HttpConfig {
            shutdown_grace: GRACE,
            ..default_config()
        };
        let bound = HttpServer::new(services, config)
            .expect("server builds")
            .bind()
            .await
            .expect("listeners bind");
        let (public, internal, ops) =
            (bound.public_addr(), bound.internal_addr(), bound.ops_addr());
        let (signal, receiver) = oneshot::channel::<()>();
        let task = tokio::spawn(bound.serve(async move {
            receiver.await.ok();
        }));
        Rig {
            public,
            internal,
            ops,
            jobs,
            signal: Some(signal),
            task,
        }
    }

    /// Sends the shutdown signal and returns the time it was sent.
    pub fn send_signal(&mut self) -> Instant {
        let sent_at = Instant::now();
        self.signal
            .take()
            .expect("the signal is sent once")
            .send(())
            .expect("server is still running");
        sent_at
    }

    /// What `run` returned, once it has.
    pub async fn finished(self) -> Result<(), HttpServerError> {
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("run returns")
            .expect("run does not panic")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    H1,
    H2,
}

/// What a request got: the status of its head, how its body ended, and whether the server closed the connection.
#[derive(Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub body: Result<Vec<u8>, hyper::Error>,
    /// When the body ended, cleanly or not.
    pub body_ended_at: Instant,
    pub connection_closed: bool,
}

enum Sender {
    H1(http1::SendRequest<Full<Bytes>>),
    H2(http2::SendRequest<Full<Bytes>>),
}

/// One client connection; the server's close shows as the end of its driver task.
pub struct Session {
    sender: Sender,
    driver: JoinHandle<()>,
}

impl Session {
    pub async fn open(addr: SocketAddr, wire: Wire) -> Session {
        let stream = TcpStream::connect(addr).await.expect("tcp connect");
        let io = TokioIo::new(stream);
        match wire {
            Wire::H1 => {
                let (sender, connection) = http1::handshake(io).await.expect("h1 handshake");
                Session {
                    sender: Sender::H1(sender),
                    driver: tokio::spawn(ended(connection)),
                }
            }
            Wire::H2 => {
                let (sender, connection) = http2::handshake(TokioExecutor::new(), io)
                    .await
                    .expect("h2 handshake");
                Session {
                    sender: Sender::H2(sender),
                    driver: tokio::spawn(ended(connection)),
                }
            }
        }
    }

    /// Sends the request and reads the answer; consumes the session, so the answer is also the last use of the
    /// connection.
    pub async fn exchange(mut self, request: Request<Full<Bytes>>) -> Reply {
        let response = match &mut self.sender {
            Sender::H1(sender) => sender.send_request(request).await,
            Sender::H2(sender) => sender.send_request(request).await,
        }
        .expect("the response head arrives");
        let (head, body) = response.into_parts();
        let body = body
            .collect()
            .await
            .map(|collected| collected.to_bytes().to_vec());
        let body_ended_at = Instant::now();
        drop(self.sender);
        let connection_closed = tokio::time::timeout(EVENT_BOUND, self.driver).await.is_ok();
        Reply {
            status: head.status,
            body,
            body_ended_at,
            connection_closed,
        }
    }
}

async fn ended<F>(connection: F)
where
    F: std::future::Future<Output = Result<(), hyper::Error>>,
{
    if let Err(error) = connection.await {
        // The peer closing is how these connections end; the reply carries the outcome that matters.
        eprintln!("client connection ended: {error}");
    }
}

pub fn claim_request() -> Request<Full<Bytes>> {
    worker_request("POST", "/api/v1/worker-jobs/claims", r#"{"lane":"image"}"#)
}

pub fn input_request(job: Uuid) -> Request<Full<Bytes>> {
    worker_request("GET", &format!("/api/v1/worker-jobs/{job}/input"), "")
}

fn worker_request(method: &str, path: &str, body: &'static str) -> Request<Full<Bytes>> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "sunnycloud.test")
        .header("authorization", format!("Bearer {WORKER_SECRET}"))
        .header("x-worker-id", WORKER)
        .body(Full::new(Bytes::from_static(body.as_bytes())))
        .expect("request")
}

/// Starts `request` on a new connection of `wire` to `addr`.
pub fn spawn_exchange(
    addr: SocketAddr,
    wire: Wire,
    request: Request<Full<Bytes>>,
) -> JoinHandle<Reply> {
    tokio::spawn(async move { Session::open(addr, wire).await.exchange(request).await })
}
