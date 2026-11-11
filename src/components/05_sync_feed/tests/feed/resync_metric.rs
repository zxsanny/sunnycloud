//! AC-8, metric half: every read answered `ResyncRequired` counts once in `feed_resync_required_total`, on the
//! immediate path and when a hold ends with it; a read that returns a page, or any other error, does not count.
//! That no log line is written per answer is checked in `tests/feed_log_audit.rs`.

use contracts::{ChangeFeed, ChangeFeedError, ResyncReason};
use sc_observability::NoLabels;
use sqlx::Connection;

use crate::hold_support::{Rig, config, cursor, ms, outcome, principal, valid};
use crate::read_support::EPOCH;

async fn owner_execute(rig: &Rig, sql: &'static str) {
    let mut conn = rig.fixture.owner_conn().await;
    sqlx::query(sql)
        .execute(&mut conn)
        .await
        .expect("owner statement");
    conn.close().await.expect("close owner connection");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_refused_cursor_counts_once_and_pages_and_other_errors_do_not() {
    // Arrange
    let rig = Rig::new(&config(2_000, 10_000), 4).await;
    let account = rig.account().await;
    let me = principal(account.id);
    rig.commit(&account).await;
    rig.commit(&account).await;
    owner_execute(&rig, "UPDATE account_feeds SET purge_horizon = 1").await;

    // Act
    let ahead = rig.feed_read_now(&me, cursor(9)).await;
    let below = rig.feed_read_now(&me, cursor(0)).await;
    let other_epoch = rig
        .feed_read_now(
            &me,
            contracts::Cursor {
                epoch: EPOCH + 1,
                seq: 1,
            },
        )
        .await;
    let page = rig.feed_read_now(&me, cursor(1)).await;
    let before_errors = rig.resyncs.get(NoLabels);
    let head = rig.feed.head(account.id).await;

    // Assert
    let reasons: Vec<_> = [ahead, below, other_epoch]
        .into_iter()
        .map(|result| match result {
            Err(ChangeFeedError::ResyncRequired { reason }) => reason,
            other => panic!("expected ResyncRequired, got {other:?}"),
        })
        .collect();
    assert_eq!(
        reasons,
        [
            ResyncReason::CursorAhead,
            ResyncReason::BelowPurgeHorizon,
            ResyncReason::EpochChanged
        ]
    );
    assert_eq!(page.expect("page").items.len(), 1);
    assert!(head.is_ok());
    assert_eq!(before_errors, 3);
    assert_eq!(rig.resyncs.get(NoLabels), 3);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hold_ended_by_an_epoch_bump_counts_once() {
    // Arrange
    let rig = Rig::new(&config(2_000, 200), 4).await;
    let account = rig.account().await;
    let held = rig.read(&principal(account.id), valid(), cursor(0), ms(30_000));
    rig.clock.sleeps_reach(1).await;
    owner_execute(&rig, "UPDATE account_feeds SET epoch = epoch + 1").await;

    // Act
    rig.clock.tick(ms(200));
    let ended = outcome(held).await;

    // Assert
    assert!(matches!(
        ended,
        Err(ChangeFeedError::ResyncRequired {
            reason: ResyncReason::EpochChanged
        })
    ));
    assert_eq!(rig.resyncs.get(NoLabels), 1);
    rig.finish().await;
}
