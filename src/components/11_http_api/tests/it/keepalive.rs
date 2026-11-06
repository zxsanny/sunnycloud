use std::time::{Duration, Instant};

use sunnycloud_http_api::HttpConfig;

use crate::harness::raw_h2::{ACK, RawH2, Read, frame_type};
use crate::harness::{Server, default_config};

const INTERVAL: Duration = Duration::from_millis(200);
const TIMEOUT: Duration = Duration::from_millis(400);
const WIRE_WAIT: Duration = Duration::from_secs(5);

fn keepalive_config() -> HttpConfig {
    HttpConfig {
        h2_keepalive_interval: INTERVAL,
        h2_keepalive_timeout: TIMEOUT,
        ..default_config()
    }
}

#[tokio::test]
async fn a_peer_that_never_answers_pings_is_pinged_then_closed_after_the_timeout() {
    // Arrange
    let server = Server::start(keepalive_config()).await;
    let mut raw = RawH2::connect(server.public).await;
    let started = Instant::now();

    // Act
    let mut first_ping = None;
    let closed_at = loop {
        match raw.read(WIRE_WAIT).await {
            Read::Frame(frame) if frame.kind == frame_type::PING && frame.flags & ACK == 0 => {
                first_ping.get_or_insert(started.elapsed());
            }
            Read::Frame(_) => {}
            Read::Closed => break started.elapsed(),
            Read::Quiet => panic!("the server neither pinged nor closed within {WIRE_WAIT:?}"),
        }
    };
    server.stop().await.expect("run returns");

    // Assert
    let first_ping = first_ping.expect("a PING arrived before the close");
    assert!(first_ping >= INTERVAL / 2, "ping after {first_ping:?}");
    assert!(
        closed_at - first_ping >= TIMEOUT / 2,
        "closed {:?} after the ping",
        closed_at - first_ping
    );
}

#[tokio::test]
async fn a_peer_that_answers_pings_stays_connected_past_the_timeout() {
    // Arrange
    let server = Server::start(keepalive_config()).await;
    let mut raw = RawH2::connect(server.public).await;
    let started = Instant::now();
    let mut answered = 0;

    // Act
    while started.elapsed() < TIMEOUT * 4 {
        match raw.read(WIRE_WAIT).await {
            Read::Frame(frame) if frame.kind == frame_type::PING && frame.flags & ACK == 0 => {
                raw.send_frame(frame_type::PING, ACK, 0, &frame.payload)
                    .await
                    .expect("the connection is still open");
                answered += 1;
            }
            Read::Frame(_) => {}
            other => panic!("the connection ended while pings were answered: {other:?}"),
        }
    }
    server.stop().await.expect("run returns");

    // Assert
    assert!(answered >= 2, "answered {answered} pings");
}
