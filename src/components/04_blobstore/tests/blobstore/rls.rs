//! AC-3: the API role sees only the scoped account's rows of `blobs` and `blob_gc`, no rows without a scope,
//! and cannot write another account's row. Rows are written as the schema owner; every read is made as
//! `sunnycloud`. The system-scope read is not probed here: see the report of AZ-3723.

use sc_db::{DbError, RunError, Scope};
use sqlx::Connection;
use uuid::Uuid;

use crate::support::{API, Fixture, account, blob, gc_row};

const COUNTS: &str = "SELECT (SELECT count(*) FROM blobs), (SELECT count(*) FROM blob_gc)";
const FOREIGN_COUNTS: &str = "SELECT (SELECT count(*) FROM blobs WHERE account_id <> $1), \
                              (SELECT count(*) FROM blob_gc WHERE account_id <> $1)";

pub struct Accounts {
    pub fixture: Fixture,
    pub a: Uuid,
    pub b: Uuid,
    pub b_blobs: Vec<Uuid>,
}

/// Each account owns a committed blob and a `gc_pending` blob with its queue entry.
pub async fn two_accounts() -> Accounts {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut b_blobs = Vec::new();
    for owner in [a, b] {
        account(&mut conn, owner).await;
        let (committed, pending) = (Uuid::new_v4(), Uuid::new_v4());
        blob(&mut conn, committed, owner, "committed", Some(10))
            .await
            .expect("committed blob");
        blob(&mut conn, pending, owner, "gc_pending", Some(10))
            .await
            .expect("pending blob");
        gc_row(&mut conn, pending, owner, "deleted")
            .await
            .expect("queue entry");
        if owner == b {
            b_blobs = vec![committed, pending];
        }
    }
    conn.close().await.expect("close owner connection");
    Accounts {
        fixture,
        a,
        b,
        b_blobs,
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
    let mut ids: Vec<Uuid> = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query_scalar("SELECT id FROM blobs")
                .fetch_all(tx.conn())
                .await
        })
        .await
        .expect("scoped read");

    // Assert
    ids.sort();
    let mut expected = accounts.b_blobs.clone();
    expected.sort();
    assert_eq!(counts, (2, 1));
    assert_eq!(ids, expected);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_b_reads_no_row_of_a() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let foreign: (i64, i64) = accounts
        .fixture
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
    assert_eq!(foreign, (0, 0));
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

    // Act
    let result = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query(
                "INSERT INTO blobs (id, account_id, state, wrapped_key, master_key_id, created_at) \
                 VALUES ($1, $2, 'uploading', '\\x00', 1, 'epoch'::timestamptz)",
            )
            .bind(Uuid::new_v4())
            .bind(accounts.a)
            .execute(tx.conn())
            .await
            .map_err(DbError::from)
        })
        .await;

    // Assert
    assert!(
        matches!(&result, Err(RunError::Closure(DbError::Other(text))) if text.starts_with("SQLSTATE 42501")),
        "{result:?}"
    );
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
         WHERE tablename IN ('blobs', 'blob_gc', 'identities') ORDER BY tablename",
    )
    .fetch_all(&mut conn)
    .await
    .expect("list policies");
    let flags: Vec<(String, bool, bool)> = sqlx::query_as(
        "SELECT relname::text, relrowsecurity, relforcerowsecurity FROM pg_class \
         WHERE relname IN ('blobs', 'blob_gc') ORDER BY relname",
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
    assert_eq!(qual("blobs"), qual("identities"));
    assert_eq!(qual("blob_gc"), qual("identities"));
    assert!(qual("blobs").contains("'system'"));
    assert_eq!(
        flags,
        [
            ("blob_gc".to_owned(), true, false),
            ("blobs".to_owned(), true, false)
        ]
    );
    drop(conn);
    fixture.finish().await;
}
