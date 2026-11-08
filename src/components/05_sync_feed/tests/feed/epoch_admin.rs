//! AC-1, AC-2, AC-3 and AC-8 (IT-05-10): the epoch bump and the head report on a real PostgreSQL 18, the bump in
//! system scope. The marker is a double: 05 may not depend on 04's file implementation.

use contracts::{EpochAdmin, Marker};
use sc_clock::Timestamp;

use crate::epoch_support::{Epochs, start_seconds};
use crate::marker_double::MarkerDouble;

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_micros(seconds * 1_000_000)
}

#[tokio::test]
async fn every_bump_sets_one_new_epoch_on_all_rows_and_the_marker_follows() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::default()).await;
    epochs.accounts(&[7, 7, 7]).await;
    let now = start_seconds();

    // Act
    let mut bumped = Vec::new();
    for _ in 0..3 {
        let epoch = epochs.admin.bump_epoch(at(now)).await.expect("bump");
        bumped.push((epoch, epochs.epochs().await, epochs.marker.value()));
    }

    // Assert
    for (index, (epoch, rows, marker)) in bumped.iter().enumerate() {
        let expected = now + i64::try_from(index).expect("index");
        assert_eq!(*epoch, expected);
        assert_eq!(rows, &vec![expected; 3]);
        assert_eq!(*marker, Some(expected));
    }
    epochs.finish().await;
}

#[tokio::test]
async fn an_aborted_bump_changes_no_row_and_leaves_the_marker() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::holding(7)).await;
    let ids = epochs.accounts(&[7, 7, 7]).await;
    epochs
        .execute(
            "CREATE FUNCTION refuse_one_bump() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN RAISE EXCEPTION 'bump refused'; END $$",
        )
        .await;
    epochs
        .execute(&format!(
            "CREATE TRIGGER refuse_one_bump BEFORE UPDATE ON account_feeds FOR EACH ROW \
             WHEN (NEW.account_id = '{}') EXECUTE FUNCTION refuse_one_bump()",
            ids[2]
        ))
        .await;

    // Act
    let aborted = epochs.admin.bump_epoch(at(start_seconds())).await;

    // Assert
    assert!(aborted.is_err());
    assert_eq!(epochs.epochs().await, vec![7, 7, 7]);
    assert_eq!(epochs.marker.value(), Some(7));
    assert_eq!(epochs.marker.writes(), 0);
    epochs.finish().await;
}

#[tokio::test]
async fn a_marker_failure_is_returned_after_the_commit_and_the_rerun_raises_the_epoch_again() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::failing()).await;
    epochs.accounts(&[7, 7]).await;
    let now = start_seconds();

    // Act
    let first = epochs.admin.bump_epoch(at(now)).await;
    let after_first = epochs.epochs().await;
    epochs.marker.set_failing(false);
    let rerun = epochs.admin.bump_epoch(at(now)).await;

    // Assert
    assert!(first.is_err());
    assert_eq!(after_first, vec![now, now]);
    assert_eq!(rerun.expect("rerun"), now + 1);
    assert_eq!(epochs.epochs().await, vec![now + 1, now + 1]);
    assert_eq!(epochs.marker.value(), Some(now + 1));
    epochs.finish().await;
}

#[tokio::test]
async fn a_clock_behind_the_epoch_still_raises_it_by_one() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::default()).await;
    let behind = start_seconds() + 1_000;
    epochs.accounts(&[behind, behind]).await;

    // Act
    let new_epoch = epochs
        .admin
        .bump_epoch(at(start_seconds()))
        .await
        .expect("bump");

    // Assert
    assert_eq!(new_epoch, behind + 1);
    assert_eq!(epochs.epochs().await, vec![behind + 1, behind + 1]);
    epochs.finish().await;
}

#[tokio::test]
async fn rows_with_different_epochs_all_take_one_epoch_above_the_highest() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::default()).await;
    epochs.accounts(&[7, 9, 8]).await;

    // Act
    let new_epoch = epochs
        .admin
        .bump_epoch(at(start_seconds()))
        .await
        .expect("bump");

    // Assert
    assert_eq!(new_epoch, start_seconds());
    assert_eq!(epochs.epochs().await, vec![start_seconds(); 3]);
    epochs.finish().await;
}

#[tokio::test]
async fn a_bump_without_any_feed_row_fails_and_writes_no_marker() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::default()).await;

    // Act
    let result = epochs.admin.bump_epoch(at(start_seconds())).await;

    // Assert
    assert!(result.is_err());
    assert_eq!(epochs.marker.writes(), 0);
    epochs.finish().await;
}

#[tokio::test]
async fn the_head_report_holds_the_highest_epoch_and_every_account_head() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::default()).await;
    let ids = epochs.accounts(&[7, 9, 8]).await;
    for (id, seq) in ids.iter().zip([10, 20, 30]) {
        epochs
            .execute(&format!(
                "UPDATE account_feeds SET seq = {seq} WHERE account_id = '{id}'"
            ))
            .await;
    }

    // Act
    let report = epochs.admin.head_report().await.expect("report");

    // Assert
    let mut expected: Vec<_> = ids
        .iter()
        .zip([(7, 10), (9, 20), (8, 30)])
        .map(|(id, (epoch, seq))| (*id, contracts::Cursor { epoch, seq }))
        .collect();
    expected.sort_by_key(|(id, _)| *id);
    assert_eq!(report.max_epoch, 9);
    assert_eq!(report.heads, expected);
    epochs.finish().await;
}

#[tokio::test]
async fn marker_status_of_an_empty_install_is_epoch_zero_and_absent() {
    // Arrange
    let epochs = Epochs::new(MarkerDouble::default()).await;

    // Act
    let status = epochs.admin.marker_status().await.expect("status");

    // Assert
    assert_eq!((status.db_epoch, status.marker), (0, Marker::Absent));
    epochs.finish().await;
}
