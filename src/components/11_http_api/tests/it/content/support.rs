use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::Router;
use axum::extract::State;
use axum::http::Method;
use axum::response::Response;
use axum::routing::get;
use contracts::{
    Disposition, Download, DownloadError, DownloadOptions, DownloadService, DownloadStatus,
    RevisionOrCurrent, StreamError,
};
use futures_core::Stream;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::{http1, http2};
use hyper::{HeaderMap, Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use sc_error::CorrelationId;
use sunnycloud_http_api::content_response;
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::harness::router_server::RouterServer;
use crate::harness::{Script, new_metrics, stub_services};

pub const OPEN: &str = "DownloadService::open_revision_content";
const ACCOUNT: Uuid = Uuid::from_u128(1);
const NODE: Uuid = Uuid::from_u128(2);
const REQUEST: CorrelationId = CorrelationId::new(Uuid::from_u128(3));

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * KIB;

/// A stream that yields its items in order.
struct Chunks(std::vec::IntoIter<Result<Bytes, StreamError>>);

impl Stream for Chunks {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.next())
    }
}

/// A `Download` of `bytes` served as `status`; the body is the bytes in 64 KiB chunks.
pub fn download(
    status: DownloadStatus,
    disposition: Disposition,
    media_type: &str,
    suggested_name: &str,
    bytes: usize,
) -> Download {
    Download {
        status,
        total_size: i64::try_from(bytes).expect("size fits"),
        range: None,
        etag: "blob-etag".to_owned(),
        media_type: Some(media_type.to_owned()),
        disposition,
        suggested_name: suggested_name.to_owned(),
        body: chunked(bytes, None),
    }
}

/// `bytes` of content in 64 KiB chunks, followed by `failure` when there is one.
pub fn chunked(bytes: usize, failure: Option<StreamError>) -> contracts::PlaintextStream {
    let mut items: Vec<Result<Bytes, StreamError>> = (0..bytes.div_ceil(64 * KIB))
        .map(|index| {
            Ok(Bytes::from(vec![
                7u8;
                (bytes - index * 64 * KIB).min(64 * KIB)
            ]))
        })
        .collect();
    items.extend(failure.map(Err));
    Box::pin(Chunks(items.into_iter()))
}

#[derive(Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("a problem body is JSON")
    }
}

/// A request that failed; `status` is set when the headers arrived before the failure.
#[derive(Debug)]
pub struct Failure {
    pub status: Option<StatusCode>,
    pub error: hyper::Error,
}

pub struct ContentServer {
    pub addr: SocketAddr,
    pub script: Script,
    server: RouterServer,
}

impl ContentServer {
    pub async fn start() -> ContentServer {
        let script = Script::default();
        let services = stub_services(&script, new_metrics());
        let app = Router::new()
            .route("/content", get(content))
            .with_state(Arc::clone(&services.downloads));
        let server = RouterServer::start(app).await;
        ContentServer {
            addr: server.addr,
            script,
            server,
        }
    }

    /// Scripts the next call of the port.
    pub fn serve(&self, result: Result<Download, DownloadError>) {
        self.script.push(OPEN, result);
    }

    /// `method /content` over a fresh HTTP/1.1 connection.
    pub async fn h1(&self, method: Method) -> Result<Reply, Failure> {
        let stream = TcpStream::connect(self.addr).await.expect("tcp connect");
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .expect("h1 handshake");
        tokio::spawn(connection);
        let response = sender
            .send_request(request(method))
            .await
            .map_err(|error| Failure {
                status: None,
                error,
            })?;
        collect(response).await
    }

    /// `method /content` over h2c with prior knowledge on a fresh connection.
    pub async fn h2(&self, method: Method) -> Result<Reply, Failure> {
        let stream = TcpStream::connect(self.addr).await.expect("tcp connect");
        let (mut sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .expect("h2 handshake");
        tokio::spawn(connection);
        let response = sender
            .send_request(request(method))
            .await
            .map_err(|error| Failure {
                status: None,
                error,
            })?;
        collect(response).await
    }

    pub async fn get(&self) -> Reply {
        self.h1(Method::GET).await.expect("an h1 answer")
    }

    pub async fn stop(self) {
        self.server.stop().await;
    }
}

fn request(method: Method) -> Request<Empty<Bytes>> {
    Request::builder()
        .method(method)
        .uri("/content")
        .header("host", "sunnycloud.test")
        .body(Empty::new())
        .expect("request")
}

async fn collect(response: hyper::Response<hyper::body::Incoming>) -> Result<Reply, Failure> {
    let (parts, body) = response.into_parts();
    let body = body
        .collect()
        .await
        .map_err(|error| Failure {
            status: Some(parts.status),
            error,
        })?
        .to_bytes()
        .to_vec();
    Ok(Reply {
        status: parts.status,
        headers: parts.headers,
        body,
    })
}

async fn content(State(downloads): State<Arc<dyn DownloadService>>, method: Method) -> Response {
    let result = downloads
        .open_revision_content(
            ACCOUNT,
            RevisionOrCurrent::Current(NODE),
            DownloadOptions::default(),
        )
        .await;
    content_response(result, &method, REQUEST)
}
