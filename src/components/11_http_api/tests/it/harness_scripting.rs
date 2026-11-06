use contracts::{NodeKind, NodeRef};
use hyper::StatusCode;
use uuid::Uuid;

use crate::harness::{Probe, clients, default_config, metric, wait_until};

const ROOT_NODE: &str = "TreeReader::root_node";
const OPEN: &str = "sunnycloud_connections_open";

#[tokio::test]
async fn a_scripted_port_result_reaches_the_route_over_h1_and_h2c() {
    // Arrange
    let probe = Probe::start(default_config()).await;
    let root = Uuid::from_u128(42);
    for _ in 0..2 {
        probe.script.push(
            ROOT_NODE,
            Ok::<_, contracts::TreeReaderError>(NodeRef {
                id: root,
                kind: NodeKind::Folder,
            }),
        );
    }

    // Act
    let over_h1 = clients::h1_get(probe.addr, "/probe/root-node").await;
    let over_h2c = clients::h2c_get(probe.addr, "/probe/root-node").await;

    // Assert
    assert_eq!(over_h1, (StatusCode::OK, root.to_string()));
    assert_eq!(over_h2c, (StatusCode::OK, root.to_string()));
    assert_eq!(probe.script.calls(ROOT_NODE), 2);
    probe.stop().await;
}

#[tokio::test]
async fn an_unscripted_port_call_fails_as_unexpected_and_not_as_a_default_success() {
    // Arrange
    let probe = Probe::start(default_config()).await;

    // Act
    let (status, body) = clients::h2c_get(probe.addr, "/probe/root-node").await;

    // Assert
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("Unexpected"), "body: {body}");
    assert!(
        body.contains(r#"UnscriptedCall("TreeReader::root_node")"#),
        "body: {body}"
    );
    assert_eq!(probe.script.calls(ROOT_NODE), 1);
    probe.stop().await;
}

#[tokio::test]
async fn a_panic_in_one_connection_task_ends_neither_the_loop_nor_the_open_gauge_accounting() {
    // Arrange
    let probe = Probe::start(default_config()).await;
    let mut doomed = clients::h1_connect(probe.addr).await;

    // Act
    let outcome = doomed
        .send_request(clients::get_request("/probe/panic"))
        .await;
    wait_until("the panicked connection to be released", || async {
        metric(&probe.metrics, OPEN).await == Some(0.0)
    })
    .await;
    let (status, _) = clients::h1_get(probe.addr, "/probe/root-node").await;

    // Assert
    assert!(outcome.is_err(), "the connection dies with the handler");
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    probe.stop().await;
}
