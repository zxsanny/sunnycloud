//! AC-4 and AC-7: the ledger runs inside the caller's transaction. A successful reserve holds the row lock to the
//! end of that transaction; release and adjust commit, or roll back to a savepoint or with the transaction,
//! together. Concurrent transactions use separate pool connections.

mod support;

use std::time::{Duration, Instant};

use contracts::QuotaLedger;
use sc_db::{Db, DbError, RunError, Scope};
use sqlx::Connection;
use support::{GIB, Harness, MIB, account_with_quota, ledger, quota_row};
use tokio::sync::oneshot;
use uuid::Uuid;

const HOLD: Duration = Duration::from_millis(500);
const HEAD_START: Duration = Duration::from_millis(100);

#[derive(Debug)]
struct Abort;

async fn hold_reservation(
    db: Db,
    account: Uuid,
    commit: bool,
    reserved: oneshot::Sender<()>,
) -> Result<(), RunError<Abort>> {
    db.run(Scope::Account(account), async |tx| {
        ledger(10 * GIB)
            .reserve(tx, account, GIB)
            .await
            .expect("reserve in A");
        reserved.send(()).expect("test is waiting");
        tokio::time::sleep(HOLD).await;
        if commit { Ok(()) } else { Err(Abort) }
    })
    .await
}

/// B's wait and B's reserved total after A ended by commit or rollback.
async fn second_reserve_after(commit: bool) -> (Duration, i64) {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, Some(10 * GIB), 0, 0).await;
    let db = harness.api_with_pool(4).await;
    let (reserved_tx, reserved_rx) = oneshot::channel();
    let first = tokio::spawn(hold_reservation(db.clone(), account, commit, reserved_tx));
    reserved_rx.await.expect("A reserved");
    tokio::time::sleep(HEAD_START).await;

    let started = Instant::now();
    let second = db
        .run(Scope::Account(account), async |tx| {
            Ok::<_, DbError>(ledger(10 * GIB).reserve(tx, account, GIB).await)
        })
        .await
        .expect("transaction B")
        .expect("reserve in B");
    let waited = started.elapsed();

    let first = first.await.expect("task A");
    assert_eq!(first.is_ok(), commit);
    assert_eq!(quota_row(&mut conn, account).await.2, second.reserved_bytes);
    conn.close().await.expect("close fixture connection");
    drop(db);
    harness.finish().await;
    (waited, second.reserved_bytes)
}

#[tokio::test]
async fn second_reserve_waits_for_a_rollback_and_only_it_is_counted() {
    // Arrange / Act
    let (waited, reserved) = second_reserve_after(false).await;

    // Assert
    assert!(waited >= Duration::from_millis(400), "{waited:?}");
    assert_eq!(reserved, GIB);
}

#[tokio::test]
async fn second_reserve_waits_for_a_commit_and_both_are_counted() {
    // Arrange / Act
    let (waited, reserved) = second_reserve_after(true).await;

    // Assert
    assert!(waited >= Duration::from_millis(400), "{waited:?}");
    assert_eq!(reserved, 2 * GIB);
}

#[derive(Clone, Copy)]
enum End {
    Commit,
    SavepointRollback,
    Abort,
}

/// Release and adjust in one transaction that ends as `end` says, with a reader sampling the row meanwhile.
/// Returns the final row and every `used + reserved` the reader saw.
async fn commit_shaped_calls(end: End) -> ((Option<i64>, i64, i64), Vec<i64>) {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, Some(10 * MIB), 0, 10 * MIB).await;
    let mut reader = harness.fixture_conn().await;
    let sampler = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..60 {
            let (_, used, reserved) = quota_row(&mut reader, account).await;
            seen.push(used + reserved);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        reader.close().await.expect("close sampler connection");
        seen
    });

    let outcome = harness
        .api
        .run(Scope::Account(account), async |tx| {
            let ledger = ledger(GIB);
            match end {
                End::SavepointRollback => {
                    let inner = tx
                        .savepoint(async |tx| {
                            ledger
                                .release_reservation(tx, account, 10 * MIB)
                                .await
                                .expect("release");
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            ledger
                                .adjust_used(tx, account, 10 * MIB)
                                .await
                                .expect("adjust");
                            Err::<(), _>(DbError::Other("rolled back".to_owned()))
                        })
                        .await;
                    assert!(inner.is_err());
                    Ok(())
                }
                End::Commit | End::Abort => {
                    ledger
                        .release_reservation(tx, account, 10 * MIB)
                        .await
                        .expect("release");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    ledger
                        .adjust_used(tx, account, 10 * MIB)
                        .await
                        .expect("adjust");
                    if matches!(end, End::Abort) {
                        Err(Abort)
                    } else {
                        Ok(())
                    }
                }
            }
        })
        .await;
    assert_eq!(outcome.is_ok(), !matches!(end, End::Abort));
    let seen = sampler.await.expect("sampler");
    let row = quota_row(&mut conn, account).await;
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
    (row, seen)
}

#[tokio::test]
async fn commit_applies_release_and_adjust_together() {
    // Arrange / Act
    let (row, seen) = commit_shaped_calls(End::Commit).await;

    // Assert
    assert_eq!(row, (Some(10 * MIB), 10 * MIB, 0));
    assert!(seen.iter().all(|total| *total <= 10 * MIB), "{seen:?}");
}

#[tokio::test]
async fn savepoint_rollback_undoes_release_and_adjust() {
    // Arrange / Act
    let (row, seen) = commit_shaped_calls(End::SavepointRollback).await;

    // Assert
    assert_eq!(row, (Some(10 * MIB), 0, 10 * MIB));
    assert!(seen.iter().all(|total| *total <= 10 * MIB), "{seen:?}");
}

#[tokio::test]
async fn aborted_transaction_undoes_release_and_adjust() {
    // Arrange / Act
    let (row, seen) = commit_shaped_calls(End::Abort).await;

    // Assert
    assert_eq!(row, (Some(10 * MIB), 0, 10 * MIB));
    assert!(seen.iter().all(|total| *total <= 10 * MIB), "{seen:?}");
}
