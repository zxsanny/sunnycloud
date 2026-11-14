//! The unit rows of AZ-3820 over hook doubles and the real 01 claim: the walk order and the clean pass before the
//! finalize (AC-1), the budget on a manual clock (AC-3), a failed hook (AC-2) and a claim held elsewhere (AC-4).

use std::time::Duration;

use contracts::Janitor;
use sc_testdb::START;
use sqlx::Connection;

use crate::purge_support::probes::{deleted_audits, state};
use crate::walk_support::{Answer, Walker};

const FREE: Duration = Duration::ZERO;

#[tokio::test]
async fn run_cycle_calls_the_next_hook_only_after_zero_and_finalizes_after_a_second_clean_pass() {
    // Arrange
    let walker = Walker::new().await;
    let account = walker.deleting_account().await;
    let hooks = walker.hooks(
        vec![
            vec![Answer::Left(3), Answer::Left(1)],
            vec![Answer::Left(2)],
            vec![],
        ],
        FREE,
    );
    let purger = walker.purger(hooks);

    // Act
    let stats = purger.run_cycle(START).await.expect("cycle");

    // Assert
    assert_eq!(stats.processed, 1);
    assert_eq!(
        walker.hook_calls(),
        [
            (0, Some(3)),
            (0, Some(1)),
            (0, Some(0)),
            (1, Some(2)),
            (1, Some(0)),
            (2, Some(0)),
            (0, Some(0)),
            (1, Some(0)),
            (2, Some(0)),
        ]
    );
    let mut conn = walker.fixture().await;
    assert_eq!(state(&mut conn, account).await, None);
    assert_eq!(deleted_audits(&mut conn, account).await, 1);
    conn.close().await.expect("close");
    walker.finish().await;
}

#[tokio::test]
async fn run_cycle_stops_at_the_budget_and_the_next_cycles_resume() {
    // Arrange
    let walker = Walker::new().await;
    let account = walker.deleting_account().await;
    let cost = walker.config.account_delete_cycle_budget / 3;
    let script = (1..=6).rev().map(Answer::Left).collect();
    let purger = walker.purger(walker.hooks(vec![script], cost));

    // Act
    let first = purger.run_cycle(START).await.expect("first cycle");
    let after_first = walker.hook_calls().len();
    let mut conn = walker.fixture().await;
    let state_after_first = state(&mut conn, account).await;
    let mut cycles = 1;
    while state(&mut conn, account).await.is_some() && cycles < 10 {
        purger.run_cycle(START).await.expect("cycle");
        cycles += 1;
    }

    // Assert
    assert_eq!(first.processed, 0);
    assert_eq!(after_first, 3, "three calls fit the budget");
    assert_eq!(state_after_first.as_deref(), Some("deleting"));
    assert!(cycles > 1);
    assert_eq!(state(&mut conn, account).await, None);
    assert_eq!(
        walker.hook_calls().len(),
        8,
        "six left, then a first pass and a clean pass of one 0 each"
    );
    conn.close().await.expect("close");
    walker.finish().await;
}

#[tokio::test]
async fn run_cycle_does_not_finalize_after_a_failed_hook_and_the_next_cycle_does() {
    // Arrange
    let walker = Walker::new().await;
    let account = walker.deleting_account().await;
    let hooks = walker.hooks(vec![vec![], vec![Answer::Fail], vec![]], FREE);
    let purger = walker.purger(hooks);

    // Act
    let first = purger.run_cycle(START).await.expect("first cycle");
    let mut conn = walker.fixture().await;
    let state_after_first = state(&mut conn, account).await;
    let second = purger.run_cycle(START).await.expect("second cycle");

    // Assert
    assert_eq!(first.processed, 0);
    assert_eq!(state_after_first.as_deref(), Some("deleting"));
    assert_eq!(second.processed, 1);
    assert_eq!(
        walker.hook_calls(),
        [
            (0, Some(0)),
            (1, None),
            (0, Some(0)),
            (1, Some(0)),
            (2, Some(0)),
            (0, Some(0)),
            (1, Some(0)),
            (2, Some(0)),
        ]
    );
    assert_eq!(deleted_audits(&mut conn, account).await, 1);
    conn.close().await.expect("close");
    walker.finish().await;
}

#[tokio::test]
async fn run_cycle_skips_an_account_whose_row_another_transaction_holds() {
    // Arrange
    let walker = Walker::new().await;
    let account = walker.deleting_account().await;
    let purger = walker.purger(walker.hooks(vec![vec![]], FREE));
    let mut holder = walker.fixture().await;
    let mut held = holder.begin().await.expect("begin");
    sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR NO KEY UPDATE")
        .bind(account)
        .execute(&mut *held)
        .await
        .expect("hold the row");

    // Act
    let while_held = purger.run_cycle(START).await.expect("cycle while held");
    let calls_while_held = walker.hook_calls().len();
    held.rollback().await.expect("release");
    let after_release = purger.run_cycle(START).await.expect("cycle after release");

    // Assert
    assert_eq!(while_held.processed, 0);
    assert_eq!(calls_while_held, 0);
    assert_eq!(after_release.processed, 1);
    let mut conn = walker.fixture().await;
    assert_eq!(state(&mut conn, account).await, None);
    conn.close().await.expect("close");
    holder.close().await.expect("close");
    walker.finish().await;
}

#[tokio::test]
async fn name_is_a_stable_non_empty_string() {
    // Arrange
    let walker = Walker::new().await;
    let purger = walker.purger(Vec::new());

    // Act
    let names = [purger.name(), purger.name()];

    // Assert
    assert!(!names[0].is_empty());
    assert_eq!(names[0], names[1]);
    assert_eq!(names[0], "deletion_purger");
    walker.finish().await;
}
