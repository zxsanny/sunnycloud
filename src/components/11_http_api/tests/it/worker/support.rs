use std::future::poll_fn;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use contracts::{
    AcceptOutcome, ByteStream, ClaimedJob, DerivedKind, FailDisposition, FailResult, Lane,
    LeaseExtended, OutputMeta, SkipOutcome, WorkerInput, WorkerJobError, WorkerJobs, WorkerStatus,
};
use futures_core::Stream;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{HeaderMap, Request};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use sunnycloud_http_api::{ApiServices, BoundServer, HttpServer};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::auth::support::{ScriptedAuthenticator, ScriptedWorkerAuth, WORKER_SECRET};
use crate::harness::script::unscripted;
use crate::harness::{EVENT_BOUND, Script, default_config, new_metrics, stub_services};

pub const JOB: Uuid = Uuid::from_u128(0xC1);
pub const WORKER: &str = "worker-7";

/// 08's `WorkerJobs` for the tests: every call is recorded as one line, the result is the next one scripted for
/// `"WorkerJobs::method"`, and a call with nothing scripted fails as `Unexpected`.
#[derive(Default)]
pub struct RecordingJobs {
    script: Script,
    calls: Mutex<Vec<String>>,
}

impl RecordingJobs {
    pub fn push<T: Send + 'static>(&self, key: &'static str, result: T) {
        self.script.push(key, result);
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, line: String) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, WorkerJobError> {
        self.script
            .take::<Result<T, WorkerJobError>>(key)
            .unwrap_or_else(|| Err(WorkerJobError::Unexpected(unscripted(key))))
    }
}

/// A one-chunk stream, as 04's reader would hand the decrypted input.
pub struct Chunk(pub Option<Bytes>);

impl Stream for Chunk {
    type Item = Result<Bytes, contracts::StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take().map(Ok))
    }
}

async fn drain(mut body: ByteStream) -> String {
    let mut bytes = Vec::new();
    while let Some(chunk) = poll_fn(|cx| body.as_mut().poll_next(cx)).await {
        bytes.extend_from_slice(&chunk.expect("the request body reaches the port intact"));
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[contracts::async_trait]
impl WorkerJobs for RecordingJobs {
    async fn claim(
        &self,
        lane: Lane,
        worker_id: &str,
    ) -> Result<Option<ClaimedJob>, WorkerJobError> {
        self.record(format!("claim {lane:?} {worker_id}"));
        self.next("WorkerJobs::claim")
    }

    async fn extend_lease(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<LeaseExtended, WorkerJobError> {
        self.record(format!("extend_lease {job_id} {worker_id}"));
        self.next("WorkerJobs::extend_lease")
    }

    async fn release(&self, job_id: Uuid, worker_id: &str) -> Result<(), WorkerJobError> {
        self.record(format!("release {job_id} {worker_id}"));
        self.next("WorkerJobs::release")
    }

    async fn complete_skipped(
        &self,
        job_id: Uuid,
        worker_id: &str,
        outcome: SkipOutcome,
    ) -> Result<(), WorkerJobError> {
        self.record(format!("complete_skipped {job_id} {worker_id} {outcome:?}"));
        self.next("WorkerJobs::complete_skipped")
    }

    async fn fail(
        &self,
        job_id: Uuid,
        worker_id: &str,
        code: &str,
        disposition: FailDisposition,
    ) -> Result<FailResult, WorkerJobError> {
        self.record(format!("fail {job_id} {worker_id} {code} {disposition:?}"));
        self.next("WorkerJobs::fail")
    }

    async fn open_input(
        &self,
        job_id: Uuid,
        worker_id: &str,
    ) -> Result<WorkerInput, WorkerJobError> {
        self.record(format!("open_input {job_id} {worker_id}"));
        self.next("WorkerJobs::open_input")
    }

    async fn accept_output(
        &self,
        job_id: Uuid,
        worker_id: &str,
        kind: DerivedKind,
        meta: OutputMeta,
        body: ByteStream,
        length: u64,
    ) -> Result<AcceptOutcome, WorkerJobError> {
        let body = drain(body).await;
        self.record(format!(
            "accept_output {job_id} {worker_id} {kind:?} {meta:?} length={length} body={body}"
        ));
        self.next("WorkerJobs::accept_output")
    }

    async fn record_status(
        &self,
        worker_id: &str,
        status: WorkerStatus,
    ) -> Result<(), WorkerJobError> {
        self.record(format!("record_status {worker_id} {status:?}"));
        self.next("WorkerJobs::record_status")
    }
}

/// The whole server: every listener of one build, over the doubles.
pub struct Stack {
    pub public: SocketAddr,
    pub internal: SocketAddr,
    #[cfg(feature = "test-clock")]
    pub test_clock: SocketAddr,
    pub jobs: Arc<RecordingJobs>,
    pub authenticator: Arc<ScriptedAuthenticator>,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Stack {
    pub async fn start() -> Stack {
        let jobs = Arc::new(RecordingJobs::default());
        let authenticator = Arc::new(ScriptedAuthenticator::default());
        let services = ApiServices {
            authenticator: authenticator.clone(),
            worker_auth: Arc::new(ScriptedWorkerAuth::default()),
            worker_jobs: jobs.clone(),
            ..stub_services(&Script::default(), new_metrics())
        };
        let bound: BoundServer = HttpServer::new(services, default_config())
            .expect("server builds")
            .bind()
            .await
            .expect("listeners bind");
        let (public, internal) = (bound.public_addr(), bound.internal_addr());
        #[cfg(feature = "test-clock")]
        let test_clock = bound.test_clock_addr();
        let (stop, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            bound
                .serve(async move {
                    signal.await.ok();
                })
                .await
                .expect("serve returns");
        });
        Stack {
            public,
            internal,
            #[cfg(feature = "test-clock")]
            test_clock,
            jobs,
            authenticator,
            stop,
            task,
        }
    }

    pub async fn stop(self) {
        self.stop.send(()).expect("server is running");
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("serve returns after the signal")
            .expect("serve does not panic");
    }
}

pub fn worker_bearer() -> String {
    format!("Bearer {WORKER_SECRET}")
}

/// What a request got back, with its headers.
pub struct Reply {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The problem body without its per-request id, to compare answers.
    pub fn problem(&self) -> Value {
        let mut body = self.json();
        body.as_object_mut()
            .expect("problem body is an object")
            .remove("request_id")
            .expect("problem body has request_id");
        body
    }

    pub fn code(&self) -> String {
        self.json()["code"].as_str().unwrap_or_default().to_owned()
    }
}

/// One request over a fresh HTTP/1.1 connection, with `body` sent as given.
pub async fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    let stream = TcpStream::connect(addr).await.expect("tcp connect");
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .expect("h1 handshake");
    tokio::spawn(connection);
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "sunnycloud.test");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder
        .body(Full::new(Bytes::copy_from_slice(body)))
        .expect("request");
    let response = sender.send_request(request).await.expect("h1 response");
    let (head, incoming) = response.into_parts();
    let body = incoming.collect().await.expect("body").to_bytes().to_vec();
    Reply {
        status: head.status.as_u16(),
        headers: head.headers,
        body,
    }
}

/// A call as the supervisor makes it: the worker bearer and its worker id.
pub async fn as_worker(
    addr: SocketAddr,
    method: &str,
    path: &str,
    extra: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    let bearer = worker_bearer();
    let mut headers = vec![("authorization", bearer.as_str()), ("x-worker-id", WORKER)];
    headers.extend_from_slice(extra);
    send(addr, method, path, &headers, body).await
}

pub fn job_path(tail: &str) -> String {
    format!("/api/v1/worker-jobs/{JOB}/{tail}")
}

/// The eight worker operations as (method, path) with a body that would be valid.
pub fn operations() -> Vec<(&'static str, String, &'static str)> {
    vec![
        (
            "POST",
            "/api/v1/worker-jobs/claims".to_owned(),
            r#"{"lane":"image"}"#,
        ),
        ("PUT", job_path("lease"), ""),
        ("DELETE", job_path("lease"), ""),
        ("GET", job_path("input"), ""),
        ("PUT", job_path("outputs/thumb"), "bytes"),
        ("PUT", job_path("result"), r#"{"outcome":"obsolete"}"#),
        (
            "POST",
            job_path("failure"),
            r#"{"code":"decode","disposition":"retry"}"#,
        ),
        (
            "PUT",
            "/api/v1/worker-jobs/status".to_owned(),
            r#"{"decoder_ok":true,"image_age_days":3}"#,
        ),
    ]
}
