//! AC-3 and ST-01-2 (AC-59, NFT-SEC-46; AZ-3692 AC-2): the runtime role `sunnycloud` sees only the scoped
//! account's rows in the four account-scoped tables, no rows without a scope, and can neither run DDL (`CREATE
//! TABLE`, `ALTER TABLE`) nor switch to the schema owner. Rows are written as the schema owner; every read is made
//! as `sunnycloud`.

mod support;

use sc_db::{DbError, RunError, Scope};
use sqlx::Connection;
use support::fixtures;
use support::{Harness, START};
use uuid::Uuid;

const COUNTS: &str = "SELECT \
    (SELECT count(*) FROM identities), (SELECT count(*) FROM sessions), \
    (SELECT count(*) FROM device_tokens), (SELECT count(*) FROM device_grants)";
const FOREIGN_COUNTS: &str = "SELECT \
    (SELECT count(*) FROM identities WHERE account_id <> $1), (SELECT count(*) FROM sessions WHERE account_id <> $1), \
    (SELECT count(*) FROM device_tokens WHERE account_id <> $1), (SELECT count(*) FROM device_grants WHERE account_id <> $1)";

struct Accounts {
    harness: Harness,
    b: Uuid,
}

async fn two_accounts() -> Accounts {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let at = START.as_unix_micros();
    for (account, label) in [(a, "a"), (b, "b")] {
        fixtures::account(&mut conn, account, at).await;
        fixtures::scoped_rows(&mut conn, account, label, at).await;
    }
    conn.close().await.expect("close fixture connection");
    Accounts { harness, b }
}

#[tokio::test]
async fn scoped_to_b_reads_one_row_per_table() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let counts: (i64, i64, i64, i64) = accounts
        .harness
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query_as(COUNTS).fetch_one(tx.conn()).await
        })
        .await
        .expect("scoped read");

    // Assert
    assert_eq!(counts, (1, 1, 1, 1));
    accounts.harness.finish().await;
}

#[tokio::test]
async fn scoped_to_b_reads_no_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let foreign: (i64, i64, i64, i64) = accounts
        .harness
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query_as(FOREIGN_COUNTS)
                .bind(accounts.b)
                .fetch_one(tx.conn())
                .await
        })
        .await
        .expect("scoped read");

    // Assert
    assert_eq!(foreign, (0, 0, 0, 0));
    accounts.harness.finish().await;
}

#[tokio::test]
async fn read_with_no_scope_set_sees_no_rows() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.harness.db.conn_as(support::API).await;

    // Act
    let counts: (i64, i64, i64, i64) = sqlx::query_as(COUNTS)
        .fetch_one(&mut conn)
        .await
        .expect("unscoped read");

    // Assert
    assert_eq!(counts, (0, 0, 0, 0));
    conn.close().await.expect("close api connection");
    accounts.harness.finish().await;
}

#[tokio::test]
async fn create_table_as_the_api_role_is_permission_denied() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let result = refused(&accounts, "CREATE TABLE intruder (id int)").await;

    // Assert
    assert!(is_permission_denied(&result), "{result:?}");
    accounts.harness.finish().await;
}

/// Runs `statement` as the API role scoped to B; the error it fails with.
async fn refused(accounts: &Accounts, statement: &'static str) -> Result<(), RunError<DbError>> {
    accounts
        .harness
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::raw_sql(statement).execute(tx.conn()).await?;
            Ok::<_, DbError>(())
        })
        .await
}

fn is_permission_denied(result: &Result<(), RunError<DbError>>) -> bool {
    matches!(result, Err(RunError::Closure(DbError::Other(text))) if text.starts_with("SQLSTATE 42501"))
}

#[tokio::test]
async fn alter_table_as_the_api_role_is_permission_denied() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let result = refused(&accounts, "ALTER TABLE sessions ADD COLUMN intruder int").await;

    // Assert
    assert!(is_permission_denied(&result), "{result:?}");
    accounts.harness.finish().await;
}

#[tokio::test]
async fn set_role_to_the_schema_owner_as_the_api_role_is_permission_denied() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let result = refused(&accounts, "SET ROLE sunnycloud_migrator").await;

    // Assert
    assert!(is_permission_denied(&result), "{result:?}");
    accounts.harness.finish().await;
}
