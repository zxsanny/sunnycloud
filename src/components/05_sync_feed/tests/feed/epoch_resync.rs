//! AC-4, AC-5 and AC-7 (IT-05-10, IT-05-11, AT-05-2): what devices see across a bump, with the real reader and the
//! real admin on PostgreSQL 18. A restore is the database state of the dump (the feed row back at seq 10); the
//! marker double keeps the value written before it.

use contracts::{ChangeFeedError, ChangePage, Cursor, EpochAdmin, Marker, ResyncReason};
use sc_clock::Timestamp;
use sunnycloud_sync_feed::PgFeedReader;
use uuid::Uuid;

use crate::epoch_support::{Epochs, start_seconds};
use crate::marker_double::MarkerDouble;
use crate::read_support::feed_config;

const OLD_EPOCH: i64 = 7;

fn bump_time() -> Timestamp {
    Timestamp::from_unix_micros(start_seconds() * 1_000_000)
}

fn reader(epochs: &Epochs) -> PgFeedReader {
    PgFeedReader::new(epochs.fixture.api.clone(), &feed_config(500, None))
}

async fn read(
    epochs: &Epochs,
    account: Uuid,
    epoch: i64,
    seq: i64,
) -> Result<ChangePage, ChangeFeedError> {
    reader(epochs)
        .read_page(account, Cursor { epoch, seq }, None)
        .await
}

fn reason(result: Result<ChangePage, ChangeFeedError>) -> ResyncReason {
    match result {
        Err(ChangeFeedError::ResyncRequired { reason }) => reason,
        other => panic!("expected ResyncRequired, got {other:?}"),
    }
}

#[tokio::test]
async fn cursors_from_before_a_bump_resync_while_a_new_cursor_reads_and_seq_is_unchanged() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::default()).await;
    let accounts = epochs.accounts(&[OLD_EPOCH, OLD_EPOCH]).await;
    epochs.execute("UPDATE account_feeds SET seq = 5").await;

    // Act
    let new_epoch = epochs.admin.bump_epoch(bump_time()).await.expect("bump");
    let old_at_head = read(&epochs, accounts[0], OLD_EPOCH, 5).await;
    let old_at_start = read(&epochs, accounts[1], OLD_EPOCH, 0).await;
    let new_cursor = read(&epochs, accounts[0], new_epoch, 0).await;

    // Assert
    assert_eq!(reason(old_at_head), ResyncReason::EpochChanged);
    assert_eq!(reason(old_at_start), ResyncReason::EpochChanged);
    assert!(new_cursor.is_ok());
    assert_eq!(epochs.seqs().await, vec![5, 5]);
    epochs.finish().await;
}

#[tokio::test]
async fn a_restore_without_a_bump_shows_cursor_ahead_and_the_marker_and_a_bump_ends_it() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::holding(OLD_EPOCH)).await;
    let account = epochs.accounts(&[OLD_EPOCH]).await[0];
    epochs.execute("UPDATE account_feeds SET seq = 10").await;

    // Act
    let ahead = read(&epochs, account, OLD_EPOCH, 40).await;
    let before = epochs.admin.marker_status().await.expect("status");
    let new_epoch = epochs.admin.bump_epoch(bump_time()).await.expect("bump");
    let after = epochs.admin.marker_status().await.expect("status");

    // Assert
    assert_eq!(reason(ahead), ResyncReason::CursorAhead);
    assert_eq!(before.db_epoch, OLD_EPOCH);
    assert_eq!(before.marker, Marker::Present(OLD_EPOCH));
    assert!(new_epoch > OLD_EPOCH);
    assert_eq!(after.db_epoch, new_epoch);
    assert_eq!(after.marker, Marker::Present(new_epoch));
    epochs.finish().await;
}

#[tokio::test]
async fn two_devices_ahead_of_a_restored_database_resync_after_the_bump_and_then_read_normally() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::holding(OLD_EPOCH)).await;
    let accounts = epochs.accounts(&[OLD_EPOCH, OLD_EPOCH]).await;
    epochs.execute("UPDATE account_feeds SET seq = 10").await;
    let (d1, d2) = (accounts[0], accounts[1]);

    // Act
    let d1_first = read(&epochs, d1, OLD_EPOCH, 40).await;
    let new_epoch = epochs.admin.bump_epoch(bump_time()).await.expect("bump");
    let d1_after = read(&epochs, d1, OLD_EPOCH, 40).await;
    let d2_after = read(&epochs, d2, OLD_EPOCH, 40).await;
    let head = reader(&epochs).head(d1).await.expect("head");
    let d1_new = read(&epochs, d1, head.epoch, head.seq).await;

    // Assert
    assert_eq!(reason(d1_first), ResyncReason::CursorAhead);
    assert_eq!(epochs.marker.value(), Some(new_epoch));
    assert_eq!(reason(d1_after), ResyncReason::EpochChanged);
    assert_eq!(reason(d2_after), ResyncReason::EpochChanged);
    assert_eq!(
        head,
        Cursor {
            epoch: new_epoch,
            seq: 10
        }
    );
    assert!(d1_new.is_ok());
    epochs.finish().await;
}
