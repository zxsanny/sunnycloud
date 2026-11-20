use std::collections::HashMap;

use contracts::{
    ChangeFeedError, ChangeItem, Cursor, NodeKind, ResyncReason, ThumbState, ThumbnailStateError,
};
use sc_ids::FileHash;
use sunnycloud_http_api::RealtimeLimits;
use tokio_tungstenite::tungstenite::protocol::Message;

use super::support::{HELLO, Realtime, change_frame, item, page, ping, subscribe};

#[tokio::test]
async fn a_subscribe_starts_the_held_read_at_its_resume_point() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let mut client = realtime.connect().await;
    assert_eq!(client.frame().await, HELLO);

    // Act
    client.send(&subscribe(1, 42)).await;
    let cursors = realtime.feed.cursors(1).await;

    // Assert
    assert_eq!(cursors, vec![Cursor { epoch: 1, seq: 42 }]);
    realtime.stop().await;
}

#[tokio::test]
async fn a_page_becomes_one_frame_per_node_the_last_of_each_seq_marked_end_and_the_read_goes_on_after_it()
 {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let mut client = realtime.connect().await;
    client.frame().await;
    client.send(&subscribe(1, 4)).await;
    realtime.feed.cursors(1).await;

    // Act
    realtime.feed.answer(Ok(page(
        vec![item(5, 1, "a"), item(5, 2, "a"), item(6, 3, "a")],
        1,
    )));
    let frames = [
        client.frame().await,
        client.frame().await,
        client.frame().await,
    ];
    let cursors = realtime.feed.cursors(2).await;

    // Assert
    assert_eq!(frames[0], change_frame(1, 5, 1, &[0x00]));
    assert_eq!(frames[1], change_frame(1, 5, 2, &[0x40]));
    assert_eq!(frames[2], change_frame(1, 6, 3, &[0x40]));
    assert_eq!(cursors[1], Cursor { epoch: 1, seq: 6 });
    realtime.stop().await;
}

#[tokio::test]
async fn a_file_frame_carries_its_size_hash_and_the_thumbnail_state_of_its_hash() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let hash = FileHash::from_bytes([7; 32]);
    realtime.script.push(
        "ThumbnailStateSource::states",
        Ok::<_, ThumbnailStateError>(HashMap::from([(hash, ThumbState::Ready)])),
    );
    let mut client = realtime.connect().await;
    client.frame().await;
    client.send(&subscribe(1, 0)).await;
    realtime.feed.cursors(1).await;
    let file = ChangeItem {
        kind: NodeKind::File,
        size: Some(300),
        content_hash: Some(hash),
        ..item(9, 8, "a")
    };

    // Act
    realtime.feed.answer(Ok(page(vec![file], 1)));
    let frame = client.frame().await;

    // Assert
    let mut expected = vec![0x21];
    expected.extend(1u64.to_le_bytes());
    expected.extend(9u64.to_le_bytes());
    expected.extend(uuid::Uuid::from_u128(8).as_bytes());
    expected.extend([0, 2, 0x02, 0x10, b'a']);
    expected.push(0b0100_1100);
    expected.extend(300u64.to_le_bytes());
    expected.extend([7; 32]);
    assert_eq!(frame, expected);
    realtime.stop().await;
}

#[tokio::test]
async fn a_ping_is_answered_with_a_pong_of_the_same_nonce() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let mut client = realtime.connect().await;
    client.frame().await;

    // Act
    client.send(&ping(0xDEAD_BEEF)).await;
    let pong = client.frame().await;

    // Assert
    assert_eq!(pong, [0x24, 0xEF, 0xBE, 0xAD, 0xDE]);
    realtime.stop().await;
}

#[tokio::test]
async fn a_second_subscribe_reads_again_from_its_own_point() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let mut client = realtime.connect().await;
    client.frame().await;
    client.send(&subscribe(1, 10)).await;
    realtime.feed.cursors(1).await;

    // Act
    client.send(&subscribe(1, 3)).await;
    let cursors = realtime.feed.cursors(2).await;

    // Assert
    assert_eq!(cursors[1], Cursor { epoch: 1, seq: 3 });
    realtime.stop().await;
}

#[tokio::test]
async fn a_resume_point_the_feed_refuses_is_a_reset_with_the_head_and_a_new_subscribe_works() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    realtime.feed.set_head(Cursor { epoch: 2, seq: 900 });
    let mut client = realtime.connect().await;
    client.frame().await;
    let reasons = [
        (ResyncReason::EpochChanged, 1u8),
        (ResyncReason::BelowPurgeHorizon, 2),
        (ResyncReason::CursorAhead, 3),
    ];

    // Act
    let mut resets = Vec::new();
    for (reason, _) in reasons {
        client.send(&subscribe(1, 5)).await;
        realtime
            .feed
            .answer(Err(ChangeFeedError::ResyncRequired { reason }));
        resets.push(client.frame().await);
    }
    client.send(&subscribe(2, 900)).await;
    let cursors = realtime.feed.cursors(4).await;

    // Assert
    for (reset, (_, number)) in resets.iter().zip(reasons) {
        let mut expected = vec![0x23, number];
        expected.extend(2u64.to_le_bytes());
        expected.extend(900u64.to_le_bytes());
        assert_eq!(reset, &expected);
    }
    assert_eq!(cursors[3], Cursor { epoch: 2, seq: 900 });
    realtime.stop().await;
}

#[tokio::test]
async fn a_revoked_session_a_busy_hold_and_a_fault_close_with_their_codes() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let expected_codes = [4401u16, 1013, 1011];
    let answers = [
        (ChangeFeedError::Unauthenticated, 4401u16),
        (
            ChangeFeedError::TooManyHolds {
                retry_after: std::time::Duration::from_secs(5),
            },
            1013,
        ),
        (
            ChangeFeedError::Unexpected(Box::new(std::io::Error::other("db down"))),
            1011,
        ),
    ];

    // Act
    let mut codes = Vec::new();
    for (error, _) in answers {
        let mut client = realtime.connect().await;
        client.frame().await;
        client.send(&subscribe(1, 0)).await;
        realtime.feed.answer(Err(error));
        codes.push(client.close_code().await);
    }

    // Assert
    let expected: Vec<Option<u16>> = expected_codes.iter().map(|code| Some(*code)).collect();
    assert_eq!(codes, expected);
    realtime.stop().await;
}

#[tokio::test]
async fn frames_that_are_not_client_frames_close_with_1002_and_text_with_1003() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let bad: [Vec<u8>; 6] = [
        vec![],
        vec![0x7e, 0],
        vec![0x20, 1, 0],
        vec![0x01, 1, 0, 0],
        [subscribe(1, 1), vec![0xff]].concat(),
        subscribe(u64::MAX, 0),
    ];

    // Act
    let mut codes = Vec::new();
    for frame in &bad {
        let mut client = realtime.connect().await;
        client.frame().await;
        client.send(frame).await;
        codes.push(client.close_code().await);
    }
    let mut text = realtime.connect().await;
    text.frame().await;
    text.send_message(Message::Text("hello".into())).await;
    let text_code = text.close_code().await;

    // Assert
    assert_eq!(codes, vec![Some(1002); bad.len()]);
    assert_eq!(text_code, Some(1003));
    realtime.stop().await;
}

#[tokio::test]
async fn a_message_above_the_limit_closes_with_1009_and_the_server_keeps_serving() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let mut big = realtime.connect().await;
    big.frame().await;
    let mut other = realtime.connect().await;
    other.frame().await;

    // Act
    big.send(&vec![0x01; 2048]).await;
    let code = big.close_code().await;
    other.send(&ping(1)).await;
    let pong = other.frame().await;

    // Assert
    assert_eq!(code, Some(1009));
    assert_eq!(pong, [0x24, 1, 0, 0, 0]);
    realtime.stop().await;
}

#[tokio::test]
async fn the_shutdown_signal_closes_every_socket_with_1001() {
    // Arrange
    let realtime = Realtime::start(RealtimeLimits::default()).await;
    let mut client = realtime.connect().await;
    client.frame().await;
    client.send(&subscribe(1, 0)).await;
    realtime.feed.cursors(1).await;

    // Act
    realtime.shutdown.cancel();
    let code = client.close_code().await;

    // Assert
    assert_eq!(code, Some(1001));
    realtime.stop().await;
}
