use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use http_body_util::{BodyExt, Empty};
use hyper::Request;
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use sunnycloud_http_api::registry::{OpenApiDocument, RouteContext};
use sunnycloud_http_api::{AcceptLoop, HttpMetrics, VersionGate};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{EVENT_BOUND, Script, default_config, new_metrics, stub_services};

/// What a request got back.
pub struct Answer {
    pub status: u16,
    pub content_type: Option<String>,
    pub content_length: Option<String>,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }
}

/// One request over a fresh HTTP/1.1 connection.
pub async fn request(addr: SocketAddr, method: &str, path: &str) -> Answer {
    request_with(addr, method, path, &[]).await
}

/// [`request`] with extra request headers.
pub async fn request_with(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> Answer {
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
    let request = builder.body(Empty::<Bytes>::new()).expect("request");
    let response = sender.send_request(request).await.expect("h1 response");
    let status = response.status().as_u16();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("header text").to_owned())
    };
    let (content_type, content_length) = (header("content-type"), header("content-length"));
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes()
        .to_vec();
    Answer {
        status,
        content_type,
        content_length,
        body,
    }
}

/// A handler context on stub ports whose ports read `script`, for mounting a registry under test. The version
/// gate has no minimum.
pub fn route_context_with(openapi: OpenApiDocument, script: &Script) -> RouteContext {
    let config = default_config();
    let metrics = new_metrics();
    let http_metrics = Arc::new(HttpMetrics::register(&metrics).expect("http families"));
    let services = stub_services(script, metrics);
    let version_gate = VersionGate::new(&config, http_metrics, services.clock.clone())
        .expect("the version gate builds");
    RouteContext {
        services: Arc::new(services),
        openapi,
        server_info: config.server_info,
        credentials: config.credentials,
        version_gate,
    }
}

/// The shared accept loop serving one router generated from a registry.
pub struct RouterServer {
    pub addr: SocketAddr,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl RouterServer {
    pub async fn start(router: Router) -> RouterServer {
        let config = default_config();
        let accept = AcceptLoop::new(&config, &new_metrics()).expect("accept loop builds");
        let listener = TcpListener::bind(config.public_addr)
            .await
            .expect("router binds");
        let addr = listener.local_addr().expect("router address");
        let stop = CancellationToken::new();
        let task = tokio::spawn({
            let stop = stop.clone();
            async move { accept.serve(listener, router, stop).await }
        });
        RouterServer { addr, stop, task }
    }

    pub async fn stop(self) {
        self.stop.cancel();
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("the accept loop stops")
            .expect("the accept loop does not panic");
    }
}
