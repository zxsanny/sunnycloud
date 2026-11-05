use std::sync::Arc;
use std::time::Duration;

use ipc_schema::*;
use tokio::io::DuplexStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::fake::{Client, FakeDaemon};
use super::scenarios;
use crate::ipc::connection::serve_connection;

fn connect(daemon: &Arc<FakeDaemon>) -> (Client<DuplexStream>, JoinHandle<()>) {
    let (client_end, server_end) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(serve_connection(Arc::clone(daemon), server_end));
    (Client::new(client_end), task)
}

#[tokio::test]
async fn token_string_is_never_on_the_wire() {
    let daemon = FakeDaemon::new();
    let (mut client, _task) = connect(&daemon);

    scenarios::token_never_on_the_wire(&mut client, &daemon).await;
}

#[tokio::test]
async fn malformed_input_is_answered_and_other_connections_keep_working() {
    let daemon = FakeDaemon::new();
    let (mut a, a_task) = connect(&daemon);
    let (mut b, _b_task) = connect(&daemon);

    scenarios::malformed_input(&mut a, &mut b).await;

    timeout(Duration::from_secs(10), a_task)
        .await
        .expect("the oversize connection must end")
        .unwrap();
}

#[tokio::test]
async fn both_subscribers_receive_both_events() {
    let daemon = FakeDaemon::new();
    let (mut a, _a_task) = connect(&daemon);
    let (mut b, _b_task) = connect(&daemon);

    scenarios::events_to_subscribers(&mut a, &mut b).await;
}

#[tokio::test]
async fn a_subscriber_that_does_not_read_is_disconnected_without_blocking_the_daemon() {
    let daemon = FakeDaemon::new();
    let (client_end, server_end) = tokio::io::duplex(1024);
    let task = tokio::spawn(serve_connection(Arc::clone(&daemon), server_end));
    let mut client = Client::new(client_end);
    let request = encode_line(
        &Request::new(RequestId::Number(1), &MethodCall::Subscribe(NoParams {})).unwrap(),
    )
    .unwrap();
    client.send_raw(&request).await.unwrap();
    scenarios::wait_for_subscribers(&daemon, 1).await;

    for _ in 0..1000 {
        daemon.emit(); // returns at once, whatever the subscriber does
    }

    timeout(Duration::from_secs(10), task)
        .await
        .expect("a stuck subscriber must be disconnected")
        .unwrap();
    daemon.emit(); // the daemon still holds the dead sink; sending to it is harmless
}

#[tokio::test]
async fn the_peer_closing_mid_line_ends_only_that_connection() {
    let daemon = FakeDaemon::new();
    let (mut a, a_task) = connect(&daemon);
    let (mut b, _b_task) = connect(&daemon);
    a.send_raw(br#"{"jsonrpc":"2.0","id":1,"meth"#)
        .await
        .unwrap();

    drop(a);

    timeout(Duration::from_secs(10), a_task)
        .await
        .unwrap()
        .unwrap();
    let status = b.call(MethodCall::Status(NoParams {})).await;
    assert!(matches!(status.outcome, Outcome::Success { .. }));
}
