use hyper::StatusCode;

use super::client::{H2, Reply};
use super::stack::Stack;
use crate::harness::{Server, default_config};

const STATUSES: [u16; 14] = [
    200, 204, 304, 400, 401, 403, 404, 405, 413, 416, 426, 429, 500, 503,
];

fn assert_fixed_headers(reply: &Reply, what: &str) {
    assert_eq!(
        reply.header("strict-transport-security"),
        "max-age=31536000",
        "{what}"
    );
    assert_eq!(reply.header("x-content-type-options"), "nosniff", "{what}");
    assert_eq!(reply.header("referrer-policy"), "no-referrer", "{what}");
    assert_eq!(reply.header("cache-control"), "no-store", "{what}");
    let cors: Vec<_> = reply
        .headers
        .keys()
        .filter(|name| name.as_str().starts_with("access-control-"))
        .collect();
    assert!(cors.is_empty(), "{what} carries {cors:?}");
}

#[tokio::test]
async fn every_status_carries_the_fixed_security_headers_and_no_cors_header() {
    // Arrange
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;

    // Act
    let mut replies = Vec::new();
    for status in STATUSES {
        let reply = connection.get(&format!("/probe/status/{status}")).await;
        replies.push((status, reply));
    }

    // Assert
    for (status, reply) in &replies {
        assert_eq!(reply.status.as_u16(), *status);
        assert_fixed_headers(reply, &format!("status {status}"));
    }
    stack.stop().await;
}

#[tokio::test]
async fn a_foreign_preflight_gets_no_cors_header_on_a_matched_or_unknown_api_path() {
    // Arrange
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;
    let foreign = [
        ("origin", "https://evil.test"),
        ("access-control-request-method", "POST"),
    ];

    // Act
    let matched = connection
        .send("OPTIONS", "/api/v1/nodes/some-node", &foreign)
        .await;
    let unknown = connection
        .send("OPTIONS", "/api/v1/not-a-route", &foreign)
        .await;

    // Assert
    assert_ne!(matched.status, StatusCode::OK);
    assert_fixed_headers(&matched, "preflight on a matched path");
    assert_fixed_headers(&unknown, "preflight on an unknown path");
    stack.stop().await;
}

#[tokio::test]
async fn a_route_that_sets_cache_control_private_keeps_it_with_the_other_fixed_headers() {
    // Arrange
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;

    // Act
    let reply = connection.get("/probe/private").await;

    // Assert
    assert_eq!(reply.header("cache-control"), "private");
    assert_eq!(reply.header("x-content-type-options"), "nosniff");
    assert_eq!(reply.header("referrer-policy"), "no-referrer");
    stack.stop().await;
}

#[tokio::test]
async fn the_real_server_applies_the_stack_to_the_public_and_internal_listeners() {
    // Arrange
    let server = Server::start(default_config()).await;
    let foreign = [
        ("origin", "https://evil.test"),
        ("access-control-request-method", "POST"),
    ];
    let mut public = H2::connect(server.public).await;
    let mut internal = H2::connect(server.internal).await;

    // Act
    let info = public.get("/api/v1/server-info").await;
    let unknown = public.get("/api/v1/nodes/some-id").await;
    let preflight = public
        .send("OPTIONS", "/api/v1/server-info", &foreign)
        .await;
    let internal_unknown = internal.get("/api/v1/server-info").await;

    // Assert
    assert_eq!(info.status, StatusCode::OK);
    assert_fixed_headers(&info, "server-info");
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert_fixed_headers(&unknown, "unknown api path");
    assert_eq!(unknown.json()["request_id"], unknown.request_id());
    assert_fixed_headers(&preflight, "preflight");
    assert_eq!(internal_unknown.status, StatusCode::NOT_FOUND);
    assert_fixed_headers(&internal_unknown, "internal listener");
    assert_eq!(
        internal_unknown.json()["request_id"],
        internal_unknown.request_id()
    );
    server.stop().await.expect("run returns");
}
