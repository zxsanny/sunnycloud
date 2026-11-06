//! AC-2: the blob migration applies, reverts and re-applies as the migrator; both tables have the columns,
//! constraints and indexes of data_model 6.1 and 6.2; the API role has DML and no DDL.

use sc_db::{DbError, Migrator, RunError, Scope};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::support::{Fixture, account, blob, gc_row, refusal};

const BLOB_VERSION: i64 = 20_261_106_120_000;

async fn columns(conn: &mut PgConnection, table: &str) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1 ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(conn)
    .await
    .expect("list columns")
}

async fn index_definition(conn: &mut PgConnection, name: &str) -> String {
    sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND indexname = $1",
    )
    .bind(name)
    .fetch_one(conn)
    .await
    .unwrap_or_else(|error| panic!("index {name}: {error}"))
}

fn column(name: &str, data_type: &str, nullable: &str) -> (String, String, String) {
    (name.to_owned(), data_type.to_owned(), nullable.to_owned())
}

#[tokio::test]
async fn blobs_has_the_columns_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let found = columns(&mut conn, "blobs").await;

    // Assert
    assert_eq!(
        found,
        [
            column("id", "uuid", "NO"),
            column("account_id", "uuid", "NO"),
            column("state", "text", "NO"),
            column("size", "bigint", "YES"),
            column("wrapped_key", "bytea", "NO"),
            column("master_key_id", "smallint", "NO"),
            column("created_at", "timestamp with time zone", "NO"),
            column("last_scrubbed_at", "timestamp with time zone", "YES"),
            column("unavailable_at", "timestamp with time zone", "YES"),
        ]
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn blob_gc_has_the_columns_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let found = columns(&mut conn, "blob_gc").await;

    // Assert
    assert_eq!(
        found,
        [
            column("blob_id", "uuid", "NO"),
            column("account_id", "uuid", "NO"),
            column("due_at", "timestamp with time zone", "NO"),
            column("reason", "text", "NO"),
        ]
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn the_indexes_are_the_ones_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let mut found = Vec::new();
    for name in [
        "blobs_id_account_id_key",
        "blobs_account_id_idx",
        "blobs_scrub_idx",
        "blob_gc_due_at_idx",
    ] {
        found.push(index_definition(&mut conn, name).await);
    }

    // Assert
    assert_eq!(
        found,
        [
            "CREATE UNIQUE INDEX blobs_id_account_id_key ON public.blobs USING btree (id, account_id)",
            "CREATE INDEX blobs_account_id_idx ON public.blobs USING btree (account_id)",
            "CREATE INDEX blobs_scrub_idx ON public.blobs USING btree (last_scrubbed_at NULLS FIRST, id) WHERE (state = 'committed'::text)",
            "CREATE INDEX blob_gc_due_at_idx ON public.blob_gc USING btree (due_at)",
        ]
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn the_constraints_refuse_what_the_data_model_forbids() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (account_a, account_b) = (Uuid::new_v4(), Uuid::new_v4());
    let blob_a = Uuid::new_v4();
    account(&mut conn, account_a).await;
    account(&mut conn, account_b).await;
    blob(&mut conn, blob_a, account_a, "committed", Some(10))
        .await
        .expect("fixture blob");
    gc_row(&mut conn, blob_a, account_a, "deleted")
        .await
        .expect("fixture queue entry");
    let queueable = Uuid::new_v4();
    blob(&mut conn, queueable, account_a, "gc_pending", None)
        .await
        .expect("a blob that is not committed needs no size");

    // Act
    let refused = [
        refusal(blob(&mut conn, Uuid::new_v4(), account_a, "bogus", None).await),
        refusal(blob(&mut conn, Uuid::new_v4(), account_a, "committed", None).await),
        refusal(blob(&mut conn, Uuid::new_v4(), account_a, "uploading", Some(-1)).await),
        refusal(blob(&mut conn, blob_a, account_b, "uploading", None).await),
        refusal(blob(&mut conn, Uuid::new_v4(), Uuid::new_v4(), "uploading", None).await),
        refusal(gc_row(&mut conn, blob_a, account_a, "deleted").await),
        refusal(gc_row(&mut conn, queueable, account_b, "deleted").await),
        refusal(gc_row(&mut conn, queueable, account_a, "forgotten").await),
    ];

    // Assert
    let expected = [
        ("23514", "blobs_state_check"),
        ("23514", "blobs_committed_size_check"),
        ("23514", "blobs_size_check"),
        ("23505", "blobs_pkey"),
        ("23503", "blobs_account_id_fkey"),
        ("23505", "blob_gc_pkey"),
        ("23503", "blob_gc_blob_id_account_id_fkey"),
        ("23514", "blob_gc_reason_check"),
    ];
    assert_eq!(
        refused,
        expected.map(|(code, constraint)| (code.to_owned(), constraint.to_owned()))
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn the_api_role_has_dml_and_cannot_alter_the_tables() {
    // Arrange
    let fixture = Fixture::new().await;
    let (account_id, blob_id) = (Uuid::new_v4(), Uuid::new_v4());
    let mut conn = fixture.owner_conn().await;
    account(&mut conn, account_id).await;
    drop(conn);

    // Act
    let dml = fixture
        .api
        .run(Scope::Account(account_id), async |tx| {
            sqlx::query(
                "INSERT INTO blobs (id, account_id, state, wrapped_key, master_key_id, created_at) \
                 VALUES ($1, $2, 'uploading', '\\x00', 1, 'epoch'::timestamptz)",
            )
            .bind(blob_id)
            .bind(account_id)
            .execute(tx.conn())
            .await?;
            sqlx::query("UPDATE blobs SET state = 'committed', size = 5 WHERE id = $1")
                .bind(blob_id)
                .execute(tx.conn())
                .await?;
            let size: i64 = sqlx::query_scalar("SELECT size FROM blobs WHERE id = $1")
                .bind(blob_id)
                .fetch_one(tx.conn())
                .await?;
            sqlx::query("INSERT INTO blob_gc (blob_id, account_id, due_at, reason) VALUES ($1, $2, 'epoch'::timestamptz, 'replaced')")
                .bind(blob_id)
                .bind(account_id)
                .execute(tx.conn())
                .await?;
            sqlx::query("DELETE FROM blob_gc WHERE blob_id = $1")
                .bind(blob_id)
                .execute(tx.conn())
                .await?;
            sqlx::query("DELETE FROM blobs WHERE id = $1")
                .bind(blob_id)
                .execute(tx.conn())
                .await?;
            Ok::<_, sqlx::Error>(size)
        })
        .await;
    let mut ddl = Vec::new();
    for statement in [
        "ALTER TABLE blobs ADD COLUMN intruder int",
        "ALTER TABLE blob_gc DROP COLUMN reason",
        "DROP TABLE blob_gc",
        "ALTER TABLE blobs DISABLE ROW LEVEL SECURITY",
    ] {
        ddl.push(
            fixture
                .api
                .run(Scope::Account(account_id), async |tx| {
                    sqlx::raw_sql(statement)
                        .execute(tx.conn())
                        .await
                        .map_err(DbError::from)
                })
                .await,
        );
    }

    // Assert
    assert!(matches!(dml, Ok(5)), "{dml:?}");
    for (index, result) in ddl.iter().enumerate() {
        assert!(
            matches!(result, Err(RunError::Closure(DbError::Other(text))) if text.starts_with("SQLSTATE 42501")),
            "statement {index}: {result:?}"
        );
    }
    fixture.finish().await;
}

#[tokio::test]
async fn up_down_up_removes_and_restores_the_tables_and_the_grants() {
    // Arrange
    let fixture = Fixture::new().await;
    let migrator = fixture.db.migrator().await;
    let status = Migrator::status(&migrator).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= BLOB_VERSION)
        .count();
    let mut conn = fixture.owner_conn().await;
    let present = "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tablename IN ('blobs', 'blob_gc')";
    let granted = "SELECT count(*) FROM information_schema.role_table_grants \
                   WHERE grantee = 'sunnycloud' AND table_name IN ('blobs', 'blob_gc')";

    // Act
    Migrator::revert(&migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the blob migration");
    let after_down: i64 = sqlx::query_scalar(present)
        .fetch_one(&mut conn)
        .await
        .expect("count");
    Migrator::run_pending(&migrator).await.expect("re-apply");
    let after_up: i64 = sqlx::query_scalar(present)
        .fetch_one(&mut conn)
        .await
        .expect("count");
    let privileges: i64 = sqlx::query_scalar(granted)
        .fetch_one(&mut conn)
        .await
        .expect("count");

    // Assert
    assert_eq!(after_down, 0);
    assert_eq!(after_up, 2);
    assert_eq!(
        privileges, 8,
        "SELECT, INSERT, UPDATE, DELETE on both tables"
    );
    conn.close().await.expect("close owner connection");
    drop(migrator);
    fixture.finish().await;
}
