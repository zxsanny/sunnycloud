//! IT-11-19, last line (AZ-3806 AC-6): a peer that is not authenticated gets its block PUT refused without a
//! body byte being read. Its own test binary, so the process memory it measures is not moved by other tests.

#[allow(dead_code)]
#[path = "it/harness/mod.rs"]
mod harness;
#[allow(dead_code)]
#[path = "it/auth/support.rs"]
mod support;

use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::to_bytes;
use axum::extract::Request;
use axum::routing::{MethodFilter, MethodRouter, on};
use http_body_util::{BodyExt, Full};
use hyper::Request as ClientRequest;
use hyper::body::Bytes;
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, RouteContext, RouteDef, RouteSpec,
};
use tokio::net::TcpStream;

use harness::router_server::RouterServer;
use support::{MEMBER_TOKEN, surface};

const BLOCK: usize = 2 << 20;
const REQUESTS: usize = 100;
const GROWTH_LIMIT_KIB: u64 = 10 * 1024;
const PATH: &str = "/api/v1/uploads/7/blocks/0";

static HANDLED: AtomicUsize = AtomicUsize::new(0);

async fn read_block(request: Request) -> String {
    HANDLED.fetch_add(1, Ordering::SeqCst);
    let body = to_bytes(request.into_body(), BLOCK + 1)
        .await
        .expect("the block arrives");
    body.len().to_string()
}

fn put_block(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, read_block)
}

/// The resident set of this process, in KiB.
fn resident_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .expect("VmRSS line")
}

async fn put(
    sender: &mut http2::SendRequest<Full<Bytes>>,
    body: &Bytes,
    token: Option<&str>,
) -> u16 {
    let mut request = ClientRequest::put(PATH)
        .header("host", "sunnycloud.test")
        .header("content-length", body.len());
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let request = request.body(Full::new(body.clone())).expect("request");
    let response = sender.send_request(request).await.expect("response");
    let status = response.status().as_u16();
    response.into_body().collect().await.expect("answer body");
    status
}

#[tokio::test]
async fn unauthenticated_block_puts_are_refused_without_reading_the_body_or_growing_memory() {
    // Arrange
    let spec = RouteSpec::new(
        HttpMethod::Put,
        "/api/v1/uploads/{upload_id}/blocks/{index}",
        "UploadService::put_block",
        AuthClass::SessionOrDevice,
        BodyClass::Block,
    )
    .summary("A test operation");
    let surface = surface(&[RouteDef::new(spec, put_block)]);
    let server = RouterServer::start(surface.routers.public).await;
    let stream = TcpStream::connect(server.addr).await.expect("tcp connect");
    let (mut sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
        .await
        .expect("h2 handshake");
    tokio::spawn(connection);
    let block = Bytes::from(vec![0xAB; BLOCK]);
    let accepted = put(&mut sender, &block, Some(MEMBER_TOKEN)).await;
    let handled_when_authenticated = HANDLED.load(Ordering::SeqCst);
    let before = resident_kib();

    // Act
    let mut statuses = Vec::new();
    for round in 0..REQUESTS {
        let token = (round % 2 == 1).then_some("revoked-session");
        statuses.push(put(&mut sender, &block, token).await);
    }
    let growth = resident_kib().saturating_sub(before);

    // Assert
    assert_eq!(accepted, 200, "the control request reaches the handler");
    assert_eq!(handled_when_authenticated, 1);
    assert!(statuses.iter().all(|status| *status == 401));
    assert_eq!(
        HANDLED.load(Ordering::SeqCst),
        1,
        "no refused request reached the handler"
    );
    assert!(
        growth < GROWTH_LIMIT_KIB,
        "resident memory grew by {growth} KiB over {REQUESTS} refused 2 MiB puts"
    );
    server.stop().await;
}
