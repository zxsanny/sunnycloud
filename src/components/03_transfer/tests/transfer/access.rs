//! AC-1 and AC-2: the transfer migration applies, reverts and re-applies as the migrator; the API role has
//! DML and no DDL; a client key is unique per account.

use sc_db::{DbError, Migrator, RunError, Scope};
use sqlx::Connection;
use uuid::Uuid;

use crate::support::{Fixture, Session, account, blob, insert_session};

const TRANSFER_VERSION: i64 = 20_261_106_140_000;
const MIB: i64 = 1024 * 1024;

#[tokio::test]
async fn a_client_key_is_unique_per_account() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let (account_a, account_b) = (Uuid::new_v4(), Uuid::new_v4());
    account(&mut conn, account_a).await;
    account(&mut conn, account_b).await;
    let mut blobs = Vec::new();
    for owner in [account_a, account_a, account_b] {
        let id = Uuid::new_v4();
        blob(&mut conn, id, owner).await;
        blobs.push(id);
    }
    drop(conn);
    let insert = |owner: Uuid, blob_id: Uuid| {
        let row = Session::new(owner, blob_id, "web:same-key", 3 * MIB);
        let api = &fixture.api;
        async move {
            api.run(Scope::Account(owner), async |tx| {
                insert_session(tx.conn(), &row).await.map_err(DbError::from)
            })
            .await
        }
    };

    // Act
    let first = insert(account_a, blobs[0]).await;
    let second_for_a = insert(account_a, blobs[1]).await;
    let for_b = insert(account_b, blobs[2]).await;

    // Assert
    assert!(first.is_ok(), "{first:?}");
    assert!(
        matches!(
            &second_for_a,
            Err(RunError::Closure(DbError::UniqueViolation { constraint: Some(name) }))
                if name == "upload_sessions_account_id_client_key_key"
        ),
        "{second_for_a:?}"
    );
    assert!(for_b.is_ok(), "{for_b:?}");
    fixture.finish().await;
}

#[tokio::test]
async fn the_api_role_has_dml_and_cannot_alter_the_tables() {
    // Arrange
    let fixture = Fixture::new().await;
    let (account_id, blob_id, session_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let mut conn = fixture.owner_conn().await;
    account(&mut conn, account_id).await;
    blob(&mut conn, blob_id, account_id).await;
    drop(conn);
    let row = Session {
        id: session_id,
        ..Session::new(account_id, blob_id, "k", 3 * MIB)
    };

    // Act
    let dml = fixture
        .api
        .run(Scope::Account(account_id), async |tx| {
            insert_session(tx.conn(), &row).await?;
            sqlx::query("UPDATE upload_sessions SET received = $2 WHERE id = $1")
                .bind(session_id)
                .bind(vec![1u8; 1])
                .execute(tx.conn())
                .await?;
            sqlx::query(
                "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
                 VALUES ($1, $2, $3, '{}'::jsonb, 'epoch'::timestamptz)",
            )
            .bind(account_id)
            .bind(Uuid::new_v4())
            .bind(vec![0u8; 32])
            .execute(tx.conn())
            .await?;
            let received: Vec<u8> =
                sqlx::query_scalar("SELECT received FROM upload_sessions WHERE id = $1")
                    .bind(session_id)
                    .fetch_one(tx.conn())
                    .await?;
            sqlx::query("DELETE FROM commit_receipts WHERE account_id = $1")
                .bind(account_id)
                .execute(tx.conn())
                .await?;
            sqlx::query("DELETE FROM upload_sessions WHERE id = $1")
                .bind(session_id)
                .execute(tx.conn())
                .await?;
            Ok::<_, sqlx::Error>(received)
        })
        .await;
    let mut ddl = Vec::new();
    for statement in [
        "ALTER TABLE upload_sessions ADD COLUMN intruder int",
        "ALTER TABLE commit_receipts DROP COLUMN response",
        "DROP TABLE commit_receipts",
        "ALTER TABLE upload_sessions DISABLE ROW LEVEL SECURITY",
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
    assert!(
        matches!(&dml, Ok(received) if received == &[1u8]),
        "{dml:?}"
    );
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
        .filter(|version| **version >= TRANSFER_VERSION)
        .count();
    let mut conn = fixture.owner_conn().await;
    let present = "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' \
                   AND tablename IN ('upload_sessions', 'commit_receipts')";
    let granted = "SELECT count(*) FROM information_schema.role_table_grants \
                   WHERE grantee = 'sunnycloud' AND table_name IN ('upload_sessions', 'commit_receipts')";
    let secured = "SELECT count(*) FROM pg_class WHERE relname IN ('upload_sessions', 'commit_receipts') \
                   AND relrowsecurity";

    // Act
    Migrator::revert(&migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the transfer migration");
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
    let with_rls: i64 = sqlx::query_scalar(secured)
        .fetch_one(&mut conn)
        .await
        .expect("count");

    // Assert
    assert!(steps >= 1, "the transfer migration is applied");
    assert_eq!(after_down, 0);
    assert_eq!(after_up, 2);
    assert_eq!(
        privileges, 8,
        "SELECT, INSERT, UPDATE, DELETE on both tables"
    );
    assert_eq!(with_rls, 2);
    conn.close().await.expect("close owner connection");
    drop(migrator);
    fixture.finish().await;
}
