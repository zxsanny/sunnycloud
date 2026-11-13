//! AC-1: both tables have the columns, keys, indexes and CHECKs of data_model 6.3 and 7.3.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::support::{Fixture, Session, account, blob, insert_session, receipt, refusal, session};

const MIB: i64 = 1024 * 1024;

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
async fn upload_sessions_has_the_columns_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let found = columns(&mut conn, "upload_sessions").await;

    // Assert
    assert_eq!(
        found,
        [
            column("id", "uuid", "NO"),
            column("account_id", "uuid", "NO"),
            column("blob_id", "uuid", "NO"),
            column("client_key", "text", "NO"),
            column("declared_size", "bigint", "NO"),
            column("block_count", "integer", "NO"),
            column("reserved_bytes", "bigint", "NO"),
            column("received", "bytea", "NO"),
            column("block_hashes", "bytea", "NO"),
            column("created_at", "timestamp with time zone", "NO"),
            column("last_checkpoint_at", "timestamp with time zone", "NO"),
            column("expires_at", "timestamp with time zone", "NO"),
            column("committed_revision_id", "uuid", "YES"),
        ]
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn commit_receipts_has_the_columns_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let found = columns(&mut conn, "commit_receipts").await;

    // Assert
    assert_eq!(
        found,
        [
            column("account_id", "uuid", "NO"),
            column("commit_id", "uuid", "NO"),
            column("request_hash", "bytea", "NO"),
            column("response", "jsonb", "NO"),
            column("created_at", "timestamp with time zone", "NO"),
        ]
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn the_keys_and_indexes_are_the_ones_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let mut found = Vec::new();
    for name in [
        "upload_sessions_pkey",
        "upload_sessions_blob_id_key",
        "upload_sessions_account_id_client_key_key",
        "upload_sessions_expires_at_idx",
        "commit_receipts_pkey",
        "commit_receipts_created_at_idx",
    ] {
        found.push(index_definition(&mut conn, name).await);
    }

    // Assert
    assert_eq!(
        found,
        [
            "CREATE UNIQUE INDEX upload_sessions_pkey ON public.upload_sessions USING btree (id)",
            "CREATE UNIQUE INDEX upload_sessions_blob_id_key ON public.upload_sessions USING btree (blob_id)",
            "CREATE UNIQUE INDEX upload_sessions_account_id_client_key_key ON public.upload_sessions USING btree (account_id, client_key)",
            "CREATE INDEX upload_sessions_expires_at_idx ON public.upload_sessions USING btree (expires_at)",
            "CREATE UNIQUE INDEX commit_receipts_pkey ON public.commit_receipts USING btree (account_id, commit_id)",
            "CREATE INDEX commit_receipts_created_at_idx ON public.commit_receipts USING btree (created_at)",
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
    account(&mut conn, account_a).await;
    account(&mut conn, account_b).await;
    let first = session(&mut conn, account_a, "k-first", 5 * MIB).await;
    let first_blob: Uuid = sqlx::query_scalar("SELECT blob_id FROM upload_sessions WHERE id = $1")
        .bind(first)
        .fetch_one(&mut conn)
        .await
        .expect("blob of the first session");
    let mut fresh = Vec::new();
    for _ in 0..8 {
        let id = Uuid::new_v4();
        blob(&mut conn, id, account_a).await;
        fresh.push(id);
    }
    let foreign_blob = Uuid::new_v4();
    blob(&mut conn, foreign_blob, account_b).await;
    let case = |blob: Uuid, key: &str, size: i64| Session::new(account_a, blob, key, size);

    // Act
    let refused = [
        refusal(insert_session(&mut conn, &case(first_blob, "k-other", MIB)).await),
        refusal(insert_session(&mut conn, &case(fresh[0], "k-first", MIB)).await),
        refusal(insert_session(&mut conn, &case(foreign_blob, "k-foreign", MIB)).await),
        refusal(insert_session(&mut conn, &case(fresh[1], "k-1", -1)).await),
        refusal(
            insert_session(
                &mut conn,
                &Session {
                    reserved_bytes: -1,
                    ..case(fresh[2], "k-2", MIB)
                },
            )
            .await,
        ),
        refusal(
            insert_session(
                &mut conn,
                &Session {
                    block_count: 2,
                    received_len: 1,
                    hashes_len: 64,
                    ..case(fresh[3], "k-3", MIB)
                },
            )
            .await,
        ),
        refusal(
            insert_session(
                &mut conn,
                &Session {
                    received_len: 2,
                    ..case(fresh[4], "k-4", 3 * MIB)
                },
            )
            .await,
        ),
        refusal(
            insert_session(
                &mut conn,
                &Session {
                    hashes_len: 31,
                    ..case(fresh[5], "k-5", MIB)
                },
            )
            .await,
        ),
        refusal(insert_session(&mut conn, &case(fresh[6], "", MIB)).await),
        refusal(insert_session(&mut conn, &case(fresh[7], &"k".repeat(257), MIB)).await),
        refusal(
            insert_session(
                &mut conn,
                &Session {
                    account: Uuid::new_v4(),
                    ..case(fresh[7], "k-8", MIB)
                },
            )
            .await,
        ),
    ];

    // Assert
    let expected = [
        ("23505", "upload_sessions_blob_id_key"),
        ("23505", "upload_sessions_account_id_client_key_key"),
        ("23503", "upload_sessions_blob_fkey"),
        ("23514", "upload_sessions_declared_size_check"),
        ("23514", "upload_sessions_reserved_bytes_check"),
        ("23514", "upload_sessions_block_count_check"),
        ("23514", "upload_sessions_received_check"),
        ("23514", "upload_sessions_block_hashes_check"),
        ("23514", "upload_sessions_client_key_check"),
        ("23514", "upload_sessions_client_key_check"),
        ("23503", "upload_sessions_account_id_fkey"),
    ];
    assert_eq!(
        refused,
        expected.map(|(code, constraint)| (code.to_owned(), constraint.to_owned()))
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn a_zero_byte_and_a_staged_row_have_the_layouts_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let owner = Uuid::new_v4();
    account(&mut conn, owner).await;

    // Act
    let accepted = [
        session(&mut conn, owner, "zero", 0).await,
        session(&mut conn, owner, "one-byte", 1).await,
        session(&mut conn, owner, "one-block", 2 * MIB).await,
        session(&mut conn, owner, "one-block-and-one-byte", 2 * MIB + 1).await,
        session(&mut conn, owner, "nine-blocks", 17 * MIB).await,
    ];
    let counts: Vec<(i64, i32)> = sqlx::query_as(
        "SELECT declared_size, block_count FROM upload_sessions ORDER BY declared_size",
    )
    .fetch_all(&mut conn)
    .await
    .expect("list rows");

    // Assert
    assert_eq!(accepted.len(), 5);
    assert_eq!(
        counts,
        [
            (0, 0),
            (1, 1),
            (2 * MIB, 1),
            (2 * MIB + 1, 2),
            (17 * MIB, 9)
        ]
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn the_receipt_key_and_hash_length_are_enforced() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (owner, other) = (Uuid::new_v4(), Uuid::new_v4());
    account(&mut conn, owner).await;
    account(&mut conn, other).await;
    let commit_id = Uuid::new_v4();
    receipt(&mut conn, owner, commit_id, 32)
        .await
        .expect("first receipt");

    // Act
    let same_commit_other_account = receipt(&mut conn, other, commit_id, 32).await;
    let refused = [
        refusal(receipt(&mut conn, owner, commit_id, 32).await),
        refusal(receipt(&mut conn, owner, Uuid::new_v4(), 31).await),
        refusal(receipt(&mut conn, Uuid::new_v4(), Uuid::new_v4(), 32).await),
    ];

    // Assert
    assert!(same_commit_other_account.is_ok());
    assert_eq!(
        refused,
        [
            ("23505".to_owned(), "commit_receipts_pkey".to_owned()),
            (
                "23514".to_owned(),
                "commit_receipts_request_hash_check".to_owned()
            ),
            (
                "23503".to_owned(),
                "commit_receipts_account_id_fkey".to_owned()
            ),
        ]
    );
    drop(conn);
    fixture.finish().await;
}

#[tokio::test]
async fn the_blob_of_an_open_session_cannot_be_removed() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let owner = Uuid::new_v4();
    account(&mut conn, owner).await;
    let id = session(&mut conn, owner, "k", MIB).await;
    let blob_id: Uuid = sqlx::query_scalar("SELECT blob_id FROM upload_sessions WHERE id = $1")
        .bind(id)
        .fetch_one(&mut conn)
        .await
        .expect("blob of the session");

    // Act
    let refused = refusal(
        sqlx::query("DELETE FROM blobs WHERE id = $1")
            .bind(blob_id)
            .execute(&mut conn)
            .await
            .map(drop),
    );
    sqlx::query("DELETE FROM upload_sessions WHERE id = $1")
        .bind(id)
        .execute(&mut conn)
        .await
        .expect("delete the session first");
    let removed = sqlx::query("DELETE FROM blobs WHERE id = $1")
        .bind(blob_id)
        .execute(&mut conn)
        .await
        .expect("delete the blob")
        .rows_affected();

    // Assert
    assert_eq!(
        refused,
        ("23001".to_owned(), "upload_sessions_blob_fkey".to_owned())
    );
    assert_eq!(removed, 1);
    drop(conn);
    fixture.finish().await;
}
