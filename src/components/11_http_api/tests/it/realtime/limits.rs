use std::time::Duration;

use sunnycloud_http_api::RealtimeLimits;
use tokio_tungstenite::tungstenite::protocol::Message;

use super::support::{Realtime, item, page, ping, subscribe};

fn short() -> RealtimeLimits {
    RealtimeLimits {
        ping_interval: Duration::from_millis(100),
        idle_timeout: Duration::from_millis(400),
        subscribe_timeout: Duration::from_millis(300),
        send_timeout: Duration::from_millis(400),
        ..RealtimeLimits::default()
    }
}

#[tokio::test]
async fn a_socket_that_never_subscribes_is_closed_4408() {
    // Arrange
    let realtime = Realtime::start(short()).await;
    let mut client = realtime.connect().await;
    client.frame().await;

    // Act
    let code = client.close_code().await;

    // Assert
    assert_eq!(code, Some(4408));
    realtime.stop().await;
}

#[tokio::test]
async fn a_subscribed_socket_that_stays_silent_is_closed_4408_and_one_that_pings_stays_open() {
    // Arrange
    let realtime = Realtime::start(short()).await;
    let mut silent = realtime.connect().await;
    silent.frame().await;
    silent.send(&subscribe(1, 0)).await;
    let mut talker = realtime.connect().await;
    talker.frame().await;
    talker.send(&subscribe(1, 0)).await;

    // Act
    for nonce in 0..8 {
        talker.send(&ping(nonce)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let silent_code = silent.close_code().await;
    talker.send(&ping(99)).await;
    let mut pongs = 0;
    while pongs < 9 {
        if let Some(Message::Binary(frame)) = talker.next().await {
            assert_eq!(frame[0], 0x24);
            pongs += 1;
        }
    }

    // Assert
    assert_eq!(silent_code, Some(4408));
    assert_eq!(pongs, 9);
    realtime.stop().await;
}

#[tokio::test]
async fn the_server_sends_a_websocket_ping_every_interval() {
    // Arrange
    let realtime = Realtime::start(short()).await;
    let mut client = realtime.connect().await;
    client.frame().await;
    client.send(&subscribe(1, 0)).await;

    // Act
    let mut pings = 0;
    while pings < 2 {
        if let Some(Message::Ping(_)) = client.next().await {
            pings += 1;
        }
    }

    // Assert
    assert_eq!(pings, 2);
    realtime.stop().await;
}

#[tokio::test]
async fn a_client_that_stops_reading_is_dropped_within_the_send_timeout_and_others_are_unaffected()
{
    // Arrange
    let limits = RealtimeLimits {
        max_connections_per_account: 2,
        idle_timeout: Duration::from_secs(60),
        subscribe_timeout: Duration::from_secs(60),
        ping_interval: Duration::from_secs(30),
        ..short()
    };
    let realtime = Realtime::start(limits).await;
    let mut stalled = realtime.connect().await;
    stalled.frame().await;
    stalled.send(&subscribe(1, 0)).await;
    realtime.feed.cursors(1).await;
    let name = "n".repeat(250);
    let items = (1..=60_000)
        .map(|seq| item(seq, u128::from(seq as u64), &name))
        .collect();
    let mut healthy = realtime.connect().await;
    healthy.frame().await;

    // Act
    realtime.feed.answer(Ok(page(items, 1)));
    healthy.send(&ping(5)).await;
    let pong = healthy.frame().await;
    let freed = async {
        loop {
            if super::support::Client::open(realtime.addr(), &super::support::member_headers())
                .await
                .is_ok()
                && super::support::Client::open(realtime.addr(), &super::support::member_headers())
                    .await
                    .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let freed = tokio::time::timeout(Duration::from_secs(10), freed).await;

    // Assert
    assert_eq!(pong, [0x24, 5, 0, 0, 0]);
    assert!(
        freed.is_ok(),
        "the stalled socket was dropped and its slot freed"
    );
    realtime.stop().await;
}
