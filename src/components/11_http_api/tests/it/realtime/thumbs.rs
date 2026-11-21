use std::sync::Arc;

use contracts::{Cursor, ThumbEvent, ThumbNotifier, ThumbState, ThumbWake};
use sc_ids::FileHash;
use sunnycloud_http_api::RealtimeLimits;

use super::support::{Realtime, TestThumbs, ping};
use crate::auth::support::{ADMIN_ACCOUNT, MEMBER_ACCOUNT};

fn event(account: uuid::Uuid, byte: u8, state: ThumbState) -> ThumbWake {
    ThumbWake::Event(ThumbEvent {
        account,
        content_hash: FileHash::from_bytes([byte; 32]),
        state,
    })
}

async fn start() -> (Realtime, Arc<TestThumbs>) {
    let thumbs = Arc::new(TestThumbs::default());
    let notifier: Arc<dyn ThumbNotifier> = thumbs.clone();
    (
        Realtime::start_with(RealtimeLimits::default(), Some(notifier)).await,
        thumbs,
    )
}

#[tokio::test]
async fn a_socket_is_registered_for_previews_before_it_sees_hello() {
    // Arrange
    let (realtime, thumbs) = start().await;
    let mut client = realtime.connect().await;

    // Act
    client.frame().await;

    // Assert
    assert_eq!(thumbs.subscribers(), 1);
    realtime.stop().await;
}

#[tokio::test]
async fn a_ready_and_a_failed_preview_of_the_account_arrive_as_thumb_state_frames() {
    // Arrange
    let (realtime, thumbs) = start().await;
    let mut client = realtime.connect().await;
    client.frame().await;

    // Act
    thumbs.wake(event(MEMBER_ACCOUNT, 7, ThumbState::Ready));
    let ready = client.frame().await;
    thumbs.wake(event(MEMBER_ACCOUNT, 8, ThumbState::Failed));
    let failed = client.frame().await;

    // Assert
    assert_eq!(ready, [vec![0x22], vec![7; 32], vec![2]].concat());
    assert_eq!(failed, [vec![0x22], vec![8; 32], vec![3]].concat());
    realtime.stop().await;
}

#[tokio::test]
async fn a_preview_of_another_account_is_not_pushed() {
    // Arrange
    let (realtime, thumbs) = start().await;
    let mut client = realtime.connect().await;
    client.frame().await;

    // Act
    thumbs.wake(event(ADMIN_ACCOUNT, 9, ThumbState::Ready));
    client.send(&ping(1)).await;
    let next = client.frame().await;

    // Assert
    assert_eq!(next, [0x24, 1, 0, 0, 0]);
    realtime.stop().await;
}

#[tokio::test]
async fn a_socket_that_lagged_gets_a_reset_with_reason_4_and_the_head() {
    // Arrange
    let (realtime, thumbs) = start().await;
    realtime.feed.set_head(Cursor { epoch: 2, seq: 77 });
    let mut client = realtime.connect().await;
    client.frame().await;

    // Act
    thumbs.wake(ThumbWake::Lagged);
    let reset = client.frame().await;

    // Assert
    let expected = [
        vec![0x23, 4],
        2u64.to_le_bytes().to_vec(),
        77u64.to_le_bytes().to_vec(),
    ]
    .concat();
    assert_eq!(reset, expected);
    realtime.stop().await;
}
