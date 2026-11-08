use std::collections::VecDeque;
use std::future::Future;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes, to_bytes};
use axum::extract::Request;
use axum::http::StatusCode;
use axum::routing::{MethodFilter, MethodRouter, on};
use contracts::{WorkerAuthError, WorkerIdentity};
use hyper::body::{Body as HttpBody, Frame};
use sc_observability::Metrics;
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, Param, ParamLocation, ParamType, RouteContext, RouteDef,
    RouteSpec, Routers, document, mount,
};
use sunnycloud_http_api::{HttpConfig, WorkerOutputLimits};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Instant, Sleep};
use uuid::Uuid;

use crate::harness::router_server::{RouterServer, route_context_for};
use crate::harness::{EVENT_BOUND, Script, default_config, metric};

/// The port every handler calls once its body is read.
pub const PORT_CALL: &str = "UploadService::abort_upload";
pub const BODY_LIMIT_METRIC: &str = "sunnycloud_body_limit_exceeded_total";

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * KIB;
pub const TIMER: Duration = Duration::from_millis(300);
pub const WORKER_THUMB_CAP: usize = KIB;
pub const WORKER_PREVIEW_CAP: usize = 8 * KIB;

pub fn path_of(class: BodyClass) -> &'static str {
    match class {
        BodyClass::Small => "/api/v1/limits/small",
        BodyClass::Commit => "/api/v1/limits/commit",
        BodyClass::Batch => "/api/v1/limits/batch",
        BodyClass::Block => "/api/v1/limits/block",
        BodyClass::Staged => "/api/v1/limits/staged",
        BodyClass::Query => "/api/v1/limits/query",
        BodyClass::Auth => "/api/v1/limits/auth",
        BodyClass::WorkerOutput => WORKER_PATH,
        BodyClass::None => "/api/v1/limits/none",
    }
}

pub const WORKER_PATH: &str = "/api/v1/worker-jobs/{job_id}/outputs/{kind}";
pub const SLOW_PATH: &str = "/api/v1/limits/slow";
pub const SLOW_BLOCK_PATH: &str = "/api/v1/limits/slow-block";
pub const POLL_ANSWERED_PATH: &str = "/api/v1/limits/poll-answered";
pub const POLL_SILENT_PATH: &str = "/api/v1/limits/poll-silent";
pub const DOWNLOAD_PATH: &str = "/api/v1/limits/download";
pub const BIG_DOWNLOAD_PATH: &str = "/api/v1/limits/big-download";
pub const BIG_DOWNLOAD_BYTES: usize = 64 * MIB;
pub const DOWNLOAD_CHUNKS: [&[u8]; 3] = [b"first-", b"second-", b"third"];
pub const STEP: Duration = Duration::from_millis(250);

async fn read_then_call(
    request: Request,
    uploads: Arc<dyn contracts::UploadService>,
) -> StatusCode {
    if to_bytes(request.into_body(), usize::MAX).await.is_err() {
        return StatusCode::BAD_REQUEST;
    }
    let outcome = uploads.abort_upload(Uuid::nil(), Uuid::nil()).await;
    if outcome.is_ok() {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    }
}

fn mount_reader(context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    let uploads = Arc::clone(&context.services.uploads);
    on(filter, move |request: Request| {
        read_then_call(request, Arc::clone(&uploads))
    })
}

fn mount_ignorer(context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    let uploads = Arc::clone(&context.services.uploads);
    on(filter, move || {
        let uploads = Arc::clone(&uploads);
        async move {
            let outcome = uploads.abort_upload(Uuid::nil(), Uuid::nil()).await;
            outcome.map_or(StatusCode::ACCEPTED, |()| StatusCode::OK)
        }
    })
}

fn mount_hang(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, std::future::pending::<StatusCode>)
}

/// Reads the whole body, then takes longer than `request_timeout` to answer.
fn mount_slow_after_body(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, |request: Request| async move {
        if to_bytes(request.into_body(), usize::MAX).await.is_err() {
            return StatusCode::BAD_REQUEST;
        }
        tokio::time::sleep(TIMER * 2).await;
        StatusCode::OK
    })
}

fn mount_answers_late(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, || async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        StatusCode::OK
    })
}

fn mount_big_download(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, || async {
        Body::from(vec![0u8; BIG_DOWNLOAD_BYTES])
    })
}

fn mount_download(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, || async {
        tokio::time::sleep(TIMER * 2).await;
        Body::new(Paced::new(DOWNLOAD_CHUNKS.to_vec()))
    })
}

/// A response body that sends one chunk per [`STEP`], so the whole stream outlasts `request_timeout`.
struct Paced {
    chunks: VecDeque<&'static [u8]>,
    sleep: Pin<Box<Sleep>>,
}

impl Paced {
    fn new(chunks: Vec<&'static [u8]>) -> Paced {
        Paced {
            chunks: chunks.into(),
            sleep: Box::pin(tokio::time::sleep(STEP)),
        }
    }
}

impl HttpBody for Paced {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        if self.chunks.is_empty() {
            return Poll::Ready(None);
        }
        if self.sleep.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        let chunk = self.chunks.pop_front().unwrap_or_default();
        self.sleep.as_mut().reset(Instant::now() + STEP);
        Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(chunk)))))
    }
}

type Mounter = fn(&RouteContext, MethodFilter) -> MethodRouter;

fn spec(method: HttpMethod, path: &'static str, class: BodyClass) -> RouteSpec {
    RouteSpec::new(
        method,
        path,
        "UploadService::abort_upload",
        AuthClass::Public,
        class,
    )
    .summary("A test operation")
}

const WAIT: &[Param] = &[Param::query("wait", ParamType::Integer)];
const OUTPUT: &[Param] = &[
    Param::id(ParamLocation::Path, "job_id", ParamType::Uuid),
    Param::path("kind", ParamType::String),
];

pub fn specs() -> Vec<(RouteSpec, Mounter)> {
    let post = |path, class| spec(HttpMethod::Post, path, class);
    let get = |path| spec(HttpMethod::Get, path, BodyClass::None);
    let mut routes: Vec<(RouteSpec, Mounter)> = [
        BodyClass::Small,
        BodyClass::Commit,
        BodyClass::Batch,
        BodyClass::Block,
        BodyClass::Staged,
        BodyClass::Query,
        BodyClass::Auth,
    ]
    .into_iter()
    .map(|class| (post(path_of(class), class), mount_reader as Mounter))
    .collect();
    routes.extend([
        (post(SLOW_PATH, BodyClass::Small), mount_hang as Mounter),
        (
            post(SLOW_BLOCK_PATH, BodyClass::Block),
            mount_slow_after_body,
        ),
        (get(path_of(BodyClass::None)), mount_ignorer),
        (
            get(POLL_ANSWERED_PATH).long_poll().params(WAIT),
            mount_answers_late,
        ),
        (get(POLL_SILENT_PATH).long_poll().params(WAIT), mount_hang),
        (get(DOWNLOAD_PATH).download(), mount_download),
        (get(BIG_DOWNLOAD_PATH).download(), mount_big_download),
        (
            RouteSpec::new(
                HttpMethod::Put,
                WORKER_PATH,
                "WorkerJobs::accept_output",
                AuthClass::Worker,
                BodyClass::WorkerOutput,
            )
            .params(OUTPUT)
            .internal()
            .summary("A test operation"),
            mount_reader,
        ),
    ]);
    routes
}

pub fn routes() -> Vec<RouteDef> {
    specs()
        .into_iter()
        .map(|(spec, mount)| RouteDef::new(spec, mount))
        .collect()
}

/// The server limits of these tests: short timers, small worker caps.
pub fn limited_config() -> HttpConfig {
    HttpConfig {
        request_timeout: TIMER,
        body_idle_timeout: TIMER,
        download_write_idle: TIMER,
        worker_output_limits: WorkerOutputLimits {
            thumb: WORKER_THUMB_CAP as u64,
            preview: WORKER_PREVIEW_CAP as u64,
            transcode: 64 * WORKER_PREVIEW_CAP as u64,
        },
        ..default_config()
    }
}

/// The routes of [`routes`] on a public and an internal listener.
pub struct Limited {
    pub public: RouterServer,
    pub internal: RouterServer,
    pub script: Script,
    metrics: Arc<Metrics>,
}

impl Limited {
    pub async fn start() -> Limited {
        let defs = routes();
        let script = Script::default();
        let context = route_context_for(limited_config(), document(&defs), &script);
        let metrics = Arc::clone(&context.services.metrics);
        let Routers { public, internal } = mount(&defs, &context, None);
        Limited {
            public: RouterServer::start_with(public, limited_config()).await,
            internal: RouterServer::start_with(internal, limited_config()).await,
            script,
            metrics,
        }
    }

    /// Lets `count` worker requests through auth.
    pub fn admit_workers(&self, count: usize) {
        for _ in 0..count {
            self.script.push(
                "WorkerAuthenticator::authenticate",
                Ok::<_, WorkerAuthError>(WorkerIdentity),
            );
        }
    }

    pub fn port_calls(&self) -> usize {
        self.script.calls(PORT_CALL)
    }

    pub async fn limit_exceeded(&self) -> f64 {
        metric(&self.metrics, BODY_LIMIT_METRIC)
            .await
            .unwrap_or(0.0)
    }

    pub async fn stop(self) {
        self.public.stop().await;
        self.internal.stop().await;
    }
}

/// A bare HTTP/1.1 connection, for requests whose head and body are sent apart or never finished.
pub struct Raw(TcpStream);

impl Raw {
    pub async fn connect(addr: SocketAddr) -> Raw {
        Raw(TcpStream::connect(addr).await.expect("tcp connect"))
    }

    pub async fn send(&mut self, bytes: &[u8]) {
        self.0.write_all(bytes).await.expect("send");
    }

    /// The head of a request that closes the connection after its answer.
    pub fn head(method: &str, path: &str, framing: &str, extra: &str) -> Vec<u8> {
        format!(
            "{method} {path} HTTP/1.1\r\nhost: sunnycloud.test\r\nconnection: close\r\n{framing}{extra}\r\n"
        )
        .into_bytes()
    }

    /// Everything the server sends until it closes the connection or resets it. Panics after `bound`.
    pub async fn finish(&mut self, bound: Duration) -> Vec<u8> {
        let mut received = Vec::new();
        let read = async {
            let mut chunk = [0u8; 8192];
            loop {
                match self.0.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(n) => received.extend_from_slice(&chunk[..n]),
                    Err(error) if error.kind() == ErrorKind::ConnectionReset => break,
                    Err(error) => panic!("read failed: {error}"),
                }
            }
        };
        tokio::time::timeout(bound, read)
            .await
            .expect("the connection ended in time");
        received
    }
}

/// The status of a raw answer, or `None` when the server sent no status line.
pub fn status_of(answer: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(answer).ok()?;
    text.strip_prefix("HTTP/1.1 ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

pub fn body_of(answer: &[u8]) -> serde_json::Value {
    let split = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the answer has a head");
    serde_json::from_slice(&answer[split + 4..]).expect("the answer body is JSON")
}

pub const ANSWER_BOUND: Duration = EVENT_BOUND;

pub fn output_path(kind: &str) -> String {
    format!("/api/v1/worker-jobs/{}/outputs/{kind}", Uuid::nil())
}
