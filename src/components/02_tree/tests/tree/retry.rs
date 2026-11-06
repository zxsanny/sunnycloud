//! AC-6: a transaction the database aborts for a deadlock or a serialization failure runs once more; a second
//! abort is the unexpected tier after exactly one retry. The deadlock is a real one between two connections.

use std::sync::atomic::{AtomicU32, Ordering};

use sc_db::{DbError, Scope};
use sqlx::Connection;
use sunnycloud_tree::{TreeError, run_retrying};
use tokio::sync::Barrier;
use uuid::Uuid;

use crate::support::{Fixture, account};

async fn account_exists(fixture: &Fixture, id: Uuid) -> bool {
    let mut conn = fixture.owner_conn().await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE id = $1")
        .bind(id)
        .fetch_one(&mut conn)
        .await
        .expect("count accounts");
    conn.close().await.expect("close fixture connection");
    count == 1
}

/// Inserts the account row inside the unit of work, then fails with `abort` for the first `aborts` attempts.
async fn insert_then_abort(
    fixture: &Fixture,
    id: Uuid,
    aborts: u32,
    abort: fn() -> TreeError,
) -> (Result<(), TreeError>, u32) {
    let attempts = AtomicU32::new(0);
    let result = run_retrying(&fixture.api, Scope::Account(id), async |tx| {
        sqlx::query(
            "INSERT INTO accounts (id, state, created_at, display_name, role) \
             VALUES ($1, 'active', 'epoch'::timestamptz, 'Retry', 'user')",
        )
        .bind(id)
        .execute(tx.conn())
        .await
        .map_err(DbError::from)?;
        if attempts.fetch_add(1, Ordering::SeqCst) < aborts {
            return Err(abort());
        }
        Ok(())
    })
    .await;
    (result, attempts.load(Ordering::SeqCst))
}

fn serialization() -> TreeError {
    TreeError::Database(DbError::Serialization)
}

#[tokio::test]
async fn success_on_the_first_attempt_runs_once() {
    // Arrange
    let fixture = Fixture::new().await;
    let id = Uuid::new_v4();

    // Act
    let (result, attempts) = insert_then_abort(&fixture, id, 0, serialization).await;

    // Assert
    result.expect("the unit of work commits");
    assert_eq!(attempts, 1);
    assert!(account_exists(&fixture, id).await);
    fixture.finish().await;
}

#[tokio::test]
async fn one_serialization_abort_succeeds_on_the_single_retry_and_the_first_attempt_is_rolled_back()
{
    // Arrange
    let fixture = Fixture::new().await;
    let id = Uuid::new_v4();

    // Act
    let (result, attempts) = insert_then_abort(&fixture, id, 1, serialization).await;

    // Assert
    result.expect("the retry commits");
    assert_eq!(attempts, 2);
    assert!(account_exists(&fixture, id).await);
    fixture.finish().await;
}

#[tokio::test]
async fn two_aborts_fail_as_the_unexpected_tier_after_exactly_one_retry() {
    // Arrange
    let fixture = Fixture::new().await;
    let id = Uuid::new_v4();

    // Act
    let (result, attempts) = insert_then_abort(&fixture, id, 2, serialization).await;

    // Assert
    let error = result.expect_err("the second abort is final");
    assert!(matches!(error, TreeError::Database(DbError::Serialization)));
    assert_eq!(attempts, 2);
    assert!(!account_exists(&fixture, id).await);
    fixture.finish().await;
}

#[tokio::test]
async fn a_business_failure_is_not_retried() {
    // Arrange
    let fixture = Fixture::new().await;
    let id = Uuid::new_v4();

    // Act
    let (result, attempts) = insert_then_abort(&fixture, id, 1, || TreeError::NameConflict).await;

    // Assert
    assert!(matches!(result, Err(TreeError::NameConflict)));
    assert_eq!(attempts, 1);
    fixture.finish().await;
}

#[tokio::test]
async fn another_database_failure_is_not_retried() {
    // Arrange
    let fixture = Fixture::new().await;
    let id = Uuid::new_v4();

    // Act
    let (result, attempts) = insert_then_abort(&fixture, id, 1, || {
        TreeError::Database(DbError::CheckViolation)
    })
    .await;

    // Assert
    assert!(matches!(
        result,
        Err(TreeError::Database(DbError::CheckViolation))
    ));
    assert_eq!(attempts, 1);
    fixture.finish().await;
}

async fn update_account(tx: &mut sc_db::DbTx, id: Uuid) -> Result<(), TreeError> {
    sqlx::query("UPDATE accounts SET state = 'active' WHERE id = $1")
        .bind(id)
        .execute(tx.conn())
        .await
        .map_err(DbError::from)?;
    Ok(())
}

/// One side of the deadlock: locks `first`, meets the other side at the barrier on its first attempt only, then
/// locks `second`. Returns the number of attempts it needed.
async fn lock_in_order(
    fixture: &Fixture,
    barrier: &Barrier,
    first: Uuid,
    second: Uuid,
) -> (Result<(), TreeError>, u32) {
    let attempts = AtomicU32::new(0);
    let result = run_retrying(&fixture.api, Scope::Account(first), async |tx| {
        let attempt = attempts.fetch_add(1, Ordering::SeqCst);
        update_account(tx, first).await?;
        if attempt == 0 {
            barrier.wait().await;
        }
        update_account(tx, second).await
    })
    .await;
    (result, attempts.load(Ordering::SeqCst))
}

#[tokio::test]
async fn a_real_deadlock_between_two_transactions_is_retried_once_and_both_commit() {
    // Arrange
    let fixture = Fixture::new().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = fixture.owner_conn().await;
    account(&mut conn, a).await;
    account(&mut conn, b).await;
    let barrier = Barrier::new(2);

    // Act
    let ((first, first_attempts), (second, second_attempts)) = tokio::join!(
        lock_in_order(&fixture, &barrier, a, b),
        lock_in_order(&fixture, &barrier, b, a)
    );

    // Assert
    first.expect("the first side commits");
    second.expect("the second side commits");
    assert_eq!(
        first_attempts + second_attempts,
        3,
        "the database aborted exactly one side, and that side ran once more"
    );
    conn.close().await.expect("close fixture connection");
    fixture.finish().await;
}
