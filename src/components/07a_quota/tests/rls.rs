//! AC-2: the runtime role sees only the scoped account's quota row, and no row without a scope. Rows are
//! written as the schema owner; every read is made as `sunnycloud`.
//!
//! The system-scope half of AC-2 (system scope sees both rows) is in `rls/system_scope_repo.rs`, the only file of
//! this crate on `scripts/ci/run-system-allowlist.txt`.

mod support;
#[path = "rls/system_scope_repo.rs"]
mod system_scope_repo;

use sc_db::Scope;
use sqlx::Connection;
use support::{Harness, account_with_quota};
use uuid::Uuid;

const READ: &str = "SELECT account_id FROM quota ORDER BY account_id";

struct Accounts {
    harness: Harness,
    a: Uuid,
    b: Uuid,
}

async fn two_accounts() -> Accounts {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    account_with_quota(&mut conn, a, Some(1000), 10, 1).await;
    account_with_quota(&mut conn, b, None, 20, 2).await;
    conn.close().await.expect("close fixture connection");
    Accounts { harness, a, b }
}

async fn visible_to(accounts: &Accounts, scope: Uuid) -> Vec<Uuid> {
    accounts
        .harness
        .api
        .run(Scope::Account(scope), async |tx| {
            sqlx::query_scalar(READ).fetch_all(tx.conn()).await
        })
        .await
        .expect("scoped read")
}

#[tokio::test]
async fn scoped_to_b_sees_only_the_row_of_b() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let visible = visible_to(&accounts, accounts.b).await;

    // Assert
    assert_eq!(visible, vec![accounts.b]);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn scoped_to_a_sees_only_the_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let visible = visible_to(&accounts, accounts.a).await;

    // Assert
    assert_eq!(visible, vec![accounts.a]);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn scoped_to_b_cannot_change_the_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let changed = accounts
        .harness
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query("UPDATE quota SET used_bytes = 0 WHERE account_id = $1")
                .bind(accounts.a)
                .execute(tx.conn())
                .await
                .map(|done| done.rows_affected())
        })
        .await
        .expect("scoped update");

    // Assert
    assert_eq!(changed, 0);
    accounts.harness.finish().await;
}

#[tokio::test]
async fn read_with_no_scope_set_sees_no_row() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.harness.db.conn_as(support::API).await;

    // Act
    let visible: Vec<Uuid> = sqlx::query_scalar(READ)
        .fetch_all(&mut conn)
        .await
        .expect("unscoped read");

    // Assert
    assert!(visible.is_empty());
    conn.close().await.expect("close api connection");
    accounts.harness.finish().await;
}

#[tokio::test]
async fn policy_admits_system_scope_next_to_the_account_match() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.harness.fixture_conn().await;

    // Act
    let policy: String = sqlx::query_scalar(
        "SELECT qual FROM pg_policies WHERE schemaname = 'public' AND tablename = 'quota'",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read the quota policy");

    // Assert
    assert!(policy.contains("'system'"), "{policy}");
    conn.close().await.expect("close fixture connection");
    accounts.harness.finish().await;
}
