//! The API role sees only the scoped account's rows of `upload_sessions` and `commit_receipts`, no rows
//! without a scope, and cannot write another account's row. Rows are written as the schema owner; every read
//! is made as `sunnycloud`. The system-scope read is not probed here: see the report of AZ-3733.

use sc_db::{DbError, RunError, Scope};
use sqlx::Connection;
use uuid::Uuid;

use crate::support::{API, Fixture, Session, account, blob, insert_session, receipt, session};

const COUNTS: &str =
    "SELECT (SELECT count(*) FROM upload_sessions), (SELECT count(*) FROM commit_receipts)";
const FOREIGN_COUNTS: &str = "SELECT (SELECT count(*) FROM upload_sessions WHERE account_id <> $1), \
                              (SELECT count(*) FROM commit_receipts WHERE account_id <> $1)";

struct Accounts {
    fixture: Fixture,
    a: Uuid,
    b: Uuid,
    b_session: Uuid,
}

/// Each account owns two sessions and one receipt.
async fn two_accounts() -> Accounts {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut b_session = Uuid::nil();
    for owner in [a, b] {
        account(&mut conn, owner).await;
        let first = session(&mut conn, owner, "one", 3 * 1024 * 1024).await;
        session(&mut conn, owner, "two", 0).await;
        receipt(&mut conn, owner, Uuid::new_v4(), 32)
            .await
            .expect("receipt");
        if owner == b {
            b_session = first;
        }
    }
    conn.close().await.expect("close owner connection");
    Accounts {
        fixture,
        a,
        b,
        b_session,
    }
}

#[tokio::test]
async fn scoped_to_b_reads_b_rows_of_both_tables() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let counts: (i64, i64) = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query_as(COUNTS).fetch_one(tx.conn()).await
        })
        .await
        .expect("scoped read");
    let by_id: Option<Uuid> = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query_scalar("SELECT id FROM upload_sessions WHERE id = $1")
                .bind(accounts.b_session)
                .fetch_optional(tx.conn())
                .await
        })
        .await
        .expect("scoped read");

    // Assert
    assert_eq!(counts, (2, 1));
    assert_eq!(by_id, Some(accounts.b_session));
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_a_reads_no_row_of_b() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let foreign: (i64, i64) = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.a), async |tx| {
            sqlx::query_as(FOREIGN_COUNTS)
                .bind(accounts.a)
                .fetch_one(tx.conn())
                .await
        })
        .await
        .expect("scoped read");
    let b_session: Option<Uuid> = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.a), async |tx| {
            sqlx::query_scalar("SELECT id FROM upload_sessions WHERE id = $1")
                .bind(accounts.b_session)
                .fetch_optional(tx.conn())
                .await
        })
        .await
        .expect("scoped read");

    // Assert
    assert_eq!(foreign, (0, 0));
    assert_eq!(b_session, None);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn read_with_no_scope_set_sees_no_rows() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.db.conn_as(API).await;

    // Act
    let counts: (i64, i64) = sqlx::query_as(COUNTS)
        .fetch_one(&mut conn)
        .await
        .expect("unscoped read");

    // Assert
    assert_eq!(counts, (0, 0));
    conn.close().await.expect("close api connection");
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_b_cannot_write_a_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;
    let mut conn = accounts.fixture.owner_conn().await;
    let blob_id = Uuid::new_v4();
    blob(&mut conn, blob_id, accounts.a).await;
    conn.close().await.expect("close owner connection");
    let row = Session::new(accounts.a, blob_id, "intruder", 1);

    // Act
    let session_result = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            insert_session(tx.conn(), &row).await.map_err(DbError::from)
        })
        .await;
    let receipt_result = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            receipt(tx.conn(), accounts.a, Uuid::new_v4(), 32)
                .await
                .map_err(DbError::from)
        })
        .await;

    // Assert
    for result in [session_result, receipt_result] {
        assert!(
            matches!(&result, Err(RunError::Closure(DbError::Other(text))) if text.starts_with("SQLSTATE 42501")),
            "{result:?}"
        );
    }
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn the_policies_have_the_text_of_the_identity_tables_and_no_force() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let policies: Vec<(String, String)> = sqlx::query_as(
        "SELECT tablename::text, qual FROM pg_policies \
         WHERE tablename IN ('upload_sessions', 'commit_receipts', 'identities') ORDER BY tablename",
    )
    .fetch_all(&mut conn)
    .await
    .expect("list policies");
    let flags: Vec<(String, bool, bool)> = sqlx::query_as(
        "SELECT relname::text, relrowsecurity, relforcerowsecurity FROM pg_class \
         WHERE relname IN ('upload_sessions', 'commit_receipts') ORDER BY relname",
    )
    .fetch_all(&mut conn)
    .await
    .expect("list flags");

    // Assert
    let qual = |table: &str| {
        policies
            .iter()
            .find(|(name, _)| name == table)
            .map(|(_, qual)| qual.clone())
            .unwrap_or_else(|| panic!("no policy on {table}"))
    };
    assert_eq!(qual("upload_sessions"), qual("identities"));
    assert_eq!(qual("commit_receipts"), qual("identities"));
    assert!(qual("upload_sessions").contains("'system'"));
    assert_eq!(
        flags,
        [
            ("commit_receipts".to_owned(), true, false),
            ("upload_sessions".to_owned(), true, false)
        ]
    );
    drop(conn);
    fixture.finish().await;
}
