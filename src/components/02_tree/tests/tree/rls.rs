//! AC-1 (row-level security): the API role sees only the scoped account's rows of `nodes` and `revisions`, no
//! rows without a scope, and cannot write another account's row. Rows are written as the schema owner; every
//! read is made as `sunnycloud`. The system-scope read is not probed here: see the report of AZ-3706.

use sc_db::{DbError, RunError, Scope};
use sqlx::Connection;
use uuid::Uuid;

use crate::support::{
    API, Fixture, RevisionRow, account_with_root, blob, file_with_revision, node_row, revision,
};

const COUNTS: &str = "SELECT (SELECT count(*) FROM nodes), (SELECT count(*) FROM revisions)";
const FOREIGN_COUNTS: &str = "SELECT (SELECT count(*) FROM nodes WHERE account_id <> $1), \
                              (SELECT count(*) FROM revisions WHERE account_id <> $1)";

struct Accounts {
    fixture: Fixture,
    a: Uuid,
    b: Uuid,
    b_root: Uuid,
}

/// Each account owns a root and a file with a current revision: two nodes and one revision.
async fn two_accounts() -> Accounts {
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut b_root = Uuid::nil();
    for owner in [a, b] {
        let root = account_with_root(&mut conn, owner).await;
        file_with_revision(&mut conn, owner, root, "f.txt").await;
        if owner == b {
            b_root = root;
        }
    }
    conn.close().await.expect("close owner connection");
    Accounts {
        fixture,
        a,
        b,
        b_root,
    }
}

fn assert_policy_refusal<T: std::fmt::Debug>(outcome: Result<T, RunError<DbError>>) {
    assert!(
        matches!(&outcome, Err(RunError::Closure(DbError::Other(text))) if text.starts_with("SQLSTATE 42501")),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn scoped_to_b_reads_the_rows_of_b_only() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let own: (i64, i64) = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.b), async |tx| {
            sqlx::query_as(COUNTS).fetch_one(tx.conn()).await
        })
        .await
        .expect("scoped read");
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
    assert_eq!(own, (2, 1));
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
async fn scoped_to_a_cannot_write_a_node_of_b() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let outcome = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.a), async |tx| {
            node_row(
                tx.conn(),
                Uuid::new_v4(),
                accounts.b,
                Some(accounts.b_root),
                "folder",
                "x",
                "x",
            )
            .await
            .map_err(DbError::from)
        })
        .await;

    // Assert
    assert_policy_refusal(outcome);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_a_cannot_write_a_revision_of_b() {
    // Arrange
    let accounts = two_accounts().await;
    let (blob_id, file) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = accounts.fixture.owner_conn().await;
    blob(&mut conn, blob_id, accounts.b).await;
    node_row(
        &mut conn,
        file,
        accounts.b,
        Some(accounts.b_root),
        "file",
        "g",
        "g",
    )
    .await
    .expect("file of b");
    conn.close().await.expect("close owner connection");

    // Act
    let outcome = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.a), async |tx| {
            revision(tx.conn(), &RevisionRow::new(accounts.b, file, blob_id))
                .await
                .map_err(DbError::from)
        })
        .await;

    // Assert
    assert_policy_refusal(outcome);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn scoped_to_a_cannot_change_or_delete_the_rows_of_b() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let (updated, deleted) = accounts
        .fixture
        .api
        .run(Scope::Account(accounts.a), async |tx| {
            let updated = sqlx::query(
                "UPDATE nodes SET updated_at = 'epoch'::timestamptz WHERE account_id = $1",
            )
            .bind(accounts.b)
            .execute(tx.conn())
            .await?
            .rows_affected();
            let deleted = sqlx::query("DELETE FROM revisions WHERE account_id = $1")
                .bind(accounts.b)
                .execute(tx.conn())
                .await?
                .rows_affected();
            Ok::<_, DbError>((updated, deleted))
        })
        .await
        .expect("statements on rows that are not visible");

    // Assert
    assert_eq!((updated, deleted), (0, 0));
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
         WHERE tablename IN ('nodes', 'revisions', 'blobs') ORDER BY tablename",
    )
    .fetch_all(&mut conn)
    .await
    .expect("list policies");

    // Assert
    let qual = |table: &str| {
        policies
            .iter()
            .find(|(name, _)| name == table)
            .map(|(_, qual)| qual.clone())
            .unwrap_or_else(|| panic!("no policy on {table}"))
    };
    assert_eq!(qual("nodes"), qual("blobs"));
    assert_eq!(qual("revisions"), qual("blobs"));
    assert!(qual("nodes").contains("'system'"));
    conn.close().await.expect("close fixture connection");
    fixture.finish().await;
}
