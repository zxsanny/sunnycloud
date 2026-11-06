use std::sync::Arc;
use std::time::Duration;

use hyper::StatusCode;
use sc_observability::{ReadinessCheck, ReadinessStatus};
use serde_json::Value;
use sunnycloud_http_api::{HttpServer, HttpServerError, Listener};
use tokio::net::TcpListener;

use crate::harness::checks::{HungCheck, SwitchCheck};
use crate::harness::clients::{get_request, h1_connect, h1_get, read_body};
use crate::harness::{self, Script, Server, default_config};

const CHECK_NAMES: [&str; 5] = [
    "database",
    "migrations",
    "key_check",
    "blob_volume",
    "scheduler",
];

fn five_checks() -> Vec<Arc<SwitchCheck>> {
    CHECK_NAMES
        .iter()
        .map(|name| SwitchCheck::ok(name))
        .collect()
}

async fn start_with(checks: &[Arc<SwitchCheck>]) -> Server {
    let readiness = checks
        .iter()
        .map(|check| Arc::clone(check) as Arc<dyn ReadinessCheck>)
        .collect();
    Server::start_with_readiness(default_config(), readiness).await
}

async fn readyz(server: &Server) -> (StatusCode, Value) {
    let (status, body) = h1_get(server.ops, "/readyz").await;
    (
        status,
        serde_json::from_str(&body).expect("readyz body is JSON"),
    )
}

fn item<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["checks"]
        .as_array()
        .expect("checks list")
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| panic!("{name} is listed"))
}

#[tokio::test]
async fn readyz_is_200_when_all_five_checks_are_ok() {
    // Arrange
    let checks = five_checks();
    let server = start_with(&checks).await;

    // Act
    let (status, body) = readyz(&server).await;

    // Assert
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["checks"].as_array().expect("checks list").len(), 5);
    server.stop().await.expect("run returns");
}

#[tokio::test]
async fn readyz_is_503_naming_each_failing_check_in_turn_without_its_internals() {
    // Arrange
    let checks = five_checks();
    let server = start_with(&checks).await;

    for (index, name) in CHECK_NAMES.iter().enumerate() {
        // Act
        checks[index].set(ReadinessStatus::Failing, "unavailable");
        let (status, body) = readyz(&server).await;

        // Assert
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{name} failing");
        assert_eq!(item(&body, name)["status"], "failing");
        assert_eq!(item(&body, name)["detail"], "unavailable");
        for other in CHECK_NAMES.iter().filter(|other| *other != name) {
            assert_eq!(item(&body, other)["status"], "ok");
        }
        checks[index].set(ReadinessStatus::Ok, "");
    }
    let (status, _) = readyz(&server).await;
    assert_eq!(status, StatusCode::OK, "recovers once every check is ok");
    server.stop().await.expect("run returns");
}

#[tokio::test]
async fn readyz_is_503_with_status_degraded_when_the_blob_volume_reports_suspect_blobs() {
    // Arrange
    let checks = five_checks();
    let server = start_with(&checks).await;
    checks[3].set(ReadinessStatus::Degraded, "suspect blobs present");

    // Act
    let (status, body) = readyz(&server).await;

    // Assert
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "degraded");
    assert_eq!(item(&body, "blob_volume")["status"], "degraded");
    server.stop().await.expect("run returns");
}

#[tokio::test]
async fn healthz_is_200_while_the_database_check_fails() {
    // Arrange
    let checks = five_checks();
    let server = start_with(&checks).await;
    checks[0].set(ReadinessStatus::Failing, "unavailable");

    // Act
    let (status, _) = h1_get(server.ops, "/healthz").await;
    let (ready, _) = readyz(&server).await;

    // Assert
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ready, StatusCode::SERVICE_UNAVAILABLE);
    server.stop().await.expect("run returns");
}

#[tokio::test]
async fn a_check_that_never_answers_makes_readyz_503_instead_of_hanging() {
    // Arrange
    let readiness: Vec<Arc<dyn ReadinessCheck>> = vec![Arc::new(HungCheck)];
    let server = Server::start_with_readiness(default_config(), readiness).await;

    // Act
    let outcome = tokio::time::timeout(Duration::from_secs(10), readyz(&server)).await;

    // Assert
    let (status, body) = outcome.expect("readyz answers although a check hangs");
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(item(&body, "scheduler")["status"], "failing");
    server.stop().await.expect("run returns");
}

#[tokio::test]
async fn ops_paths_answer_404_on_the_public_and_internal_listeners_and_api_paths_404_on_ops() {
    // Arrange
    let server = Server::start(default_config()).await;
    let ops_paths = ["/healthz", "/readyz", "/metrics"];

    // Act
    let mut off_ops = Vec::new();
    for addr in [server.public, server.internal] {
        for path in ops_paths {
            off_ops.push(h1_get(addr, path).await.0);
        }
    }
    let api_on_ops = h1_get(server.ops, "/api/v1/nodes/0").await.0;
    let mut on_ops = Vec::new();
    for path in ops_paths {
        on_ops.push(h1_get(server.ops, path).await.0);
    }

    // Assert
    assert!(
        off_ops
            .iter()
            .all(|status| *status == StatusCode::NOT_FOUND)
    );
    assert_eq!(api_on_ops, StatusCode::NOT_FOUND);
    assert!(on_ops.iter().all(|status| *status == StatusCode::OK));
    server.stop().await.expect("run returns");
}

#[tokio::test]
async fn the_shutdown_signal_turns_readyz_to_503_on_a_connection_opened_before_it() {
    // Arrange
    let checks = five_checks();
    let server = start_with(&checks).await;
    let mut ops = h1_connect(server.ops).await;
    let before = ops
        .send_request(get_request("/readyz"))
        .await
        .expect("readyz response");
    assert_eq!(read_body(before).await.0, StatusCode::OK);

    // Act
    server.stop().await.expect("run returns");
    let after = ops
        .send_request(get_request("/readyz"))
        .await
        .expect("readyz response after the signal");

    // Assert
    let (status, body) = read_body(after).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("shutting down"));
}

#[tokio::test]
async fn a_taken_ops_address_fails_before_serving_with_the_ops_listener_named() {
    // Arrange
    let taken = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("occupy a port");
    let mut config = default_config();
    config.ops_addr = taken.local_addr().expect("taken address");
    let server = HttpServer::new(
        harness::stub_services(&Script::default(), harness::new_metrics()),
        config,
    )
    .expect("server builds");

    // Act
    let outcome = server.bind().await;

    // Assert
    assert!(matches!(
        outcome,
        Err(HttpServerError::Bind {
            listener: Listener::Ops,
            ..
        })
    ));
}
