//! AC-2: the runtime role sees only the scoped account's feed row, no row without a scope, cannot change
//! another account's row and is refused DDL. Rows are written as the schema owner; every read is made as
//! `sunnycloud`.
//!
//! The system-scope half of AC-2 (system scope sees both rows) is in `system_scope_repo.rs`, the only file of
//! this crate on `scripts/ci/run-system-allowlist.txt`.

use sc_db::Scope;
use sqlx::Connection;
use uuid::Uuid;

use crate::support::{API, Fixture, account_with_feed, sqlstate};

const PERMISSION_DENIED: &str = "42501";
const READ: &str = "SELECT account_id FROM account_feeds ORDER BY account_id";

pub struct Accounts {
    pub fixture: Fixture,
    pub a: Uuid,
    pub b: Uuid,
}

pub async fn two_accounts() -> Accounts {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    account_with_feed(&mut conn, a, 100).await;
    account_with_feed(&mut conn, b, 200).await;
    conn.close().await.expect("close owner connection");
    Accounts { fixture, a, b }
}

async fn visible_to(accounts: &Accounts, scope: Uuid) -> Vec<Uuid> {
    accounts
        .fixture
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
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_a_sees_only_the_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let visible = visible_to(&accounts, accounts.a).await;

    // Assert
    assert_eq!(visible, vec![accounts.a]);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_b_cannot_change_the_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let changed = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query("UPDATE account_feeds SET seq = seq + 1 WHERE account_id = $1")
                .bind(accounts.a)
                .execute(tx.conn())
                .await
                .map(|done| done.rows_affected())
        })
        .await
        .expect("scoped update");

    // Assert
    assert_eq!(changed, 0);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn read_with_no_scope_set_sees_no_row() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.db.conn_as(API).await;

    // Act
    let visible: Vec<Uuid> = sqlx::query_scalar(READ)
        .fetch_all(&mut conn)
        .await
        .expect("unscoped read");

    // Assert
    assert!(visible.is_empty());
    conn.close().await.expect("close api connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn runtime_role_cannot_alter_or_truncate_the_table() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.db.conn_as(API).await;

    // Act
    let alter = sqlx::raw_sql("ALTER TABLE account_feeds ADD COLUMN extra bigint")
        .execute(&mut conn)
        .await;
    let truncate = sqlx::raw_sql("TRUNCATE account_feeds")
        .execute(&mut conn)
        .await;
    let alter_nodes = sqlx::raw_sql("ALTER TABLE nodes DROP COLUMN seq")
        .execute(&mut conn)
        .await;

    // Assert
    for result in [alter, truncate, alter_nodes] {
        let error = result.expect_err("DDL is refused");
        assert_eq!(sqlstate(&error), PERMISSION_DENIED);
    }
    conn.close().await.expect("close api connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn policy_admits_system_scope_next_to_the_account_match() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;

    // Act
    let policy: String = sqlx::query_scalar(
        "SELECT qual FROM pg_policies WHERE schemaname = 'public' AND tablename = 'account_feeds'",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read the feed policy");

    // Assert
    assert!(policy.contains("'system'"), "{policy}");
    conn.close().await.expect("close owner connection");
    accounts.fixture.finish().await;
}
