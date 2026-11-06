use hyper::StatusCode;
use sunnycloud_http_api::{HttpServer, HttpServerError, Listener};
use tokio::net::{TcpListener, TcpStream};

use crate::harness::{self, Script, Server, clients, default_config};

#[tokio::test]
async fn public_and_internal_listeners_serve_h1_and_h2c_until_the_signal_and_run_returns() {
    // Arrange
    let server = Server::start(default_config()).await;

    // Act
    let h1_public = clients::h1_get(server.public, "/anything").await;
    let h2c_public = clients::h2c_get(server.public, "/anything").await;
    let h1_internal = clients::h1_get(server.internal, "/anything").await;
    let h2c_internal = clients::h2c_get(server.internal, "/anything").await;
    let ports_called = server.script.calls("TreeReader::root_node");
    let public = server.public;
    let outcome = server.stop().await;

    // Assert
    for (status, _) in [h1_public, h2c_public, h1_internal, h2c_internal] {
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    assert!(outcome.is_ok());
    assert_eq!(ports_called, 0, "an unrouted path reaches no port");
    assert!(
        TcpStream::connect(public).await.is_err(),
        "the listener is closed after run returns"
    );
}

#[tokio::test]
async fn run_binds_the_configured_addresses_and_returns_after_the_signal() {
    // Arrange
    let script = Script::default();
    let server = HttpServer::new(
        harness::stub_services(&script, harness::new_metrics()),
        default_config(),
    )
    .expect("server builds");

    // Act
    let outcome = server.run(std::future::ready(())).await;

    // Assert
    assert!(outcome.is_ok());
}

#[tokio::test]
async fn a_taken_public_address_fails_before_serving_with_the_listener_named() {
    // Arrange
    let taken = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("occupy a port");
    let mut config = default_config();
    config.public_addr = taken.local_addr().expect("taken address");
    let server = HttpServer::new(
        harness::stub_services(&Script::default(), harness::new_metrics()),
        config,
    )
    .expect("server builds");

    // Act
    let outcome = server.run(std::future::pending()).await;

    // Assert
    assert!(matches!(
        outcome,
        Err(HttpServerError::Bind {
            listener: Listener::Public,
            ..
        })
    ));
}

#[tokio::test]
async fn a_second_server_on_the_same_metrics_registry_is_refused() {
    // Arrange
    let script = Script::default();
    let metrics = harness::new_metrics();
    let _first = HttpServer::new(
        harness::stub_services(&script, metrics.clone()),
        default_config(),
    )
    .expect("first server builds");

    // Act
    let second = HttpServer::new(harness::stub_services(&script, metrics), default_config());

    // Assert
    assert!(matches!(second, Err(HttpServerError::Metrics(_))));
}

#[tokio::test]
async fn two_openapi_calls_return_identical_bytes() {
    // Arrange
    let server = HttpServer::new(
        harness::stub_services(&Script::default(), harness::new_metrics()),
        default_config(),
    )
    .expect("server builds");

    // Act
    let first = server.openapi();
    let second = server.openapi();

    // Assert
    assert_eq!(first, second);
    assert!(!first.as_bytes().is_empty());
}

#[tokio::test]
async fn a_zero_connection_cap_is_refused_by_new() {
    // Arrange
    let mut config = default_config();
    config.max_connections = 0;

    // Act
    let result = HttpServer::new(
        harness::stub_services(&Script::default(), harness::new_metrics()),
        config,
    );

    // Assert
    assert!(matches!(result, Err(HttpServerError::InvalidConfig(_))));
}
