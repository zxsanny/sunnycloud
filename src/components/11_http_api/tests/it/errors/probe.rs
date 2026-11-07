use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use contracts::{NodeSelector, TreeReader};
use http_body_util::{BodyExt, Empty};
use hyper::Request;
use hyper::body::Bytes;
use hyper::header::{HeaderMap as ReplyHeaders, HeaderValue as ReplyValue};
use hyper_util::rt::TokioIo;
use sc_error::CorrelationId;
use serde_json::Value;
use sunnycloud_http_api::{AcceptLoop, ApiProblem};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::cases;
use crate::harness::{EVENT_BOUND, Script, default_config, new_metrics, stub_services};

pub const ACCOUNT: Uuid = Uuid::from_u128(1);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A request id no other request of this test binary uses.
pub fn fresh_id() -> CorrelationId {
    let serial = u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    CorrelationId::new(Uuid::from_u128(
        0x0198_d5a1_0000_7000_8000_0000_0000_0000 | serial,
    ))
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: ReplyHeaders<ReplyValue>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a problem body is JSON")
    }

    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("header {name} is missing"))
            .to_str()
            .expect("header text")
    }
}

/// The accept loop serving routes whose handlers answer through [`ApiProblem`].
pub struct ErrorProbe {
    pub addr: SocketAddr,
    pub script: Script,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl ErrorProbe {
    pub async fn start() -> ErrorProbe {
        let config = default_config();
        let script = Script::default();
        let metrics = new_metrics();
        let services = stub_services(&script, metrics.clone());
        let accept = AcceptLoop::new(&config, &metrics).expect("accept loop builds");
        let listener = TcpListener::bind(config.public_addr)
            .await
            .expect("probe binds");
        let addr = listener.local_addr().expect("probe address");
        let stop = CancellationToken::new();
        let app = Router::new()
            .route("/probe/errors/{case}", get(case))
            .route("/probe/nodes/{id}", get(node))
            .with_state(Arc::clone(&services.tree_reader));
        let task = tokio::spawn({
            let stop = stop.clone();
            async move { accept.serve(listener, app, stop).await }
        });
        ErrorProbe {
            addr,
            script,
            stop,
            task,
        }
    }

    /// `GET path` on a fresh HTTP/1.1 connection, sending `id` as `X-Request-Id`.
    pub async fn get(&self, path: &str, id: CorrelationId) -> Reply {
        let stream = TcpStream::connect(self.addr).await.expect("tcp connect");
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .expect("h1 handshake");
        tokio::spawn(connection);
        let request = Request::get(path)
            .header("host", "sunnycloud.test")
            .header("x-request-id", id.to_string())
            .body(Empty::<Bytes>::new())
            .expect("request");
        let response = sender.send_request(request).await.expect("h1 response");
        let (parts, body) = response.into_parts();
        let body = body.collect().await.expect("body").to_bytes().to_vec();
        Reply {
            status: parts.status,
            headers: parts.headers,
            body,
        }
    }

    pub async fn stop(self) {
        self.stop.cancel();
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("the accept loop stops")
            .expect("the accept loop does not panic");
    }
}

fn request_id(headers: &HeaderMap) -> Option<CorrelationId> {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .and_then(CorrelationId::parse)
}

async fn case(Path(name): Path<String>, headers: HeaderMap) -> Response {
    let Some(id) = request_id(&headers) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match cases::build(&name, id) {
        Some(problem) => problem.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn node(
    State(tree): State<Arc<dyn TreeReader>>,
    Path(node): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let Some(id) = request_id(&headers) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match tree.get_node(ACCOUNT, NodeSelector::Id(node)).await {
        Ok(view) => view.id.to_string().into_response(),
        Err(error) => ApiProblem::from_port(error, id).into_response(),
    }
}
