//! AC-1 and AC-4 of the admin lifecycle: the row is created with the account in the caller's transaction and
//! erased idempotently. Accounts are created through `initialize`, never by a direct insert.

mod support;

use contracts::{AccountLifecycleHook, LifecycleError, QuotaAdminError, QuotaView};
use sc_db::{DbError, RunError, Scope};
use sqlx::Connection;
use support::{GIB, Harness, TIB, account, admin, has_row, in_tx, initialized, quota_row};
use uuid::Uuid;

const DEFAULT: i64 = 10 * GIB;

#[tokio::test]
async fn committed_initialize_creates_an_unset_empty_row_with_the_default_as_effective_limit() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let id = Uuid::new_v4();
    account(&mut conn, id).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    in_tx(&harness.api, id, async |tx| {
        quota.initialize(tx, id).await.expect("initialize")
    })
    .await;

    // Assert
    assert_eq!(quota_row(&mut conn, id).await, (None, 0, 0));
    assert_eq!(
        quota.view(id).await.unwrap(),
        QuotaView {
            limit_bytes: None,
            effective_limit: DEFAULT,
            used: 0,
            reserved: 0
        }
    );
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn rolled_back_initialize_leaves_no_row() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let id = Uuid::new_v4();
    account(&mut conn, id).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let outcome = harness
        .api
        .run(Scope::Account(id), async |tx| {
            quota.initialize(tx, id).await.expect("initialize");
            Err::<(), _>("the account creation failed later")
        })
        .await;

    // Assert
    assert!(matches!(outcome, Err(RunError::Closure(_))));
    assert!(!has_row(&mut conn, id).await);
    assert!(matches!(
        quota.view(id).await,
        Err(QuotaAdminError::NotFound)
    ));
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn initialize_twice_is_unexpected_and_keeps_the_first_row() {
    // Arrange
    let harness = Harness::new().await;
    let id = Uuid::new_v4();
    initialized(&harness, DEFAULT, id, Some(GIB), 5, 0).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let second = harness
        .api
        .run(Scope::Account(id), async |tx| {
            quota.initialize(tx, id).await
        })
        .await;

    // Assert
    assert!(matches!(
        second,
        Err(RunError::Closure(LifecycleError::Unexpected(source))) if source.downcast_ref::<DbError>()
            .is_some_and(|error| matches!(error, DbError::UniqueViolation { .. }))
    ));
    assert_eq!(quota.view(id).await.unwrap().used, 5);
    harness.finish().await;
}

#[tokio::test]
async fn erase_removes_the_row_with_its_reservations_and_repeats_and_unknown_accounts_report_zero()
{
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (erased, other, never) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    initialized(&harness, DEFAULT, erased, Some(TIB), 0, 5 * GIB).await;
    initialized(&harness, DEFAULT, other, Some(TIB), 7, 3).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let first = quota.erase_batch(erased, 50).await;
    let repeat = quota.erase_batch(erased, 50).await;
    let unknown = quota.erase_batch(never, 50).await;

    // Assert
    assert_eq!(first.unwrap().remaining, 0);
    assert_eq!(repeat.unwrap().remaining, 0);
    assert_eq!(unknown.unwrap().remaining, 0);
    assert!(!has_row(&mut conn, erased).await);
    assert_eq!(quota_row(&mut conn, other).await, (Some(TIB), 7, 3));
    assert!(matches!(
        quota.view(erased).await,
        Err(QuotaAdminError::NotFound)
    ));
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}
