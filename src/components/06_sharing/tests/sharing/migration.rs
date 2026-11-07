//! AC-1: the share migration applies, reverts and re-applies as the migrator role and creates `share_links` of
//! data_model 8.1 with its unique token hash, two indexes and the composite foreign key; the runtime role gets DML
//! only.

use sc_db::{Db, Migrator};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::harness::{Harness, account_with_file, link, sqlstate};

const SHARE_VERSION: i64 = 20_261_106_190_000;
const UNIQUE_VIOLATION: &str = "23505";
const CHECK_VIOLATION: &str = "23514";
const FOREIGN_KEY_VIOLATION: &str = "23503";

type Column = (String, String, String);

/// Reverts the share migration and everything applied after it.
async fn revert_share(migrator: &Db) {
    let status = Migrator::status(migrator).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= SHARE_VERSION)
        .count();
    Migrator::revert(migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the share migration");
}

async fn exists(conn: &mut PgConnection) -> bool {
    sqlx::query_scalar("SELECT to_regclass('public.share_links') IS NOT NULL")
        .fetch_one(conn)
        .await
        .expect("look up share_links")
}

async fn columns(conn: &mut PgConnection) -> Vec<Column> {
    sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'share_links' ORDER BY ordinal_position",
    )
    .fetch_all(conn)
    .await
    .expect("list columns")
}

async fn index_names(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT indexname::text FROM pg_indexes WHERE schemaname = 'public' AND tablename = 'share_links' \
         ORDER BY indexname",
    )
    .fetch_all(conn)
    .await
    .expect("list indexes")
}

async fn constraint_definitions(conn: &mut PgConnection, kind: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
         WHERE conrelid = 'public.share_links'::regclass AND contype = $1::\"char\" ORDER BY conname",
    )
    .bind(kind)
    .fetch_all(conn)
    .await
    .expect("list constraints")
}

fn expected_columns() -> Vec<Column> {
    let column = |name: &str, data_type: &str, nullable: &str| {
        (name.to_owned(), data_type.to_owned(), nullable.to_owned())
    };
    vec![
        column("id", "uuid", "NO"),
        column("account_id", "uuid", "NO"),
        column("node_id", "uuid", "NO"),
        column("token_hash", "bytea", "NO"),
        column("password_hash", "text", "YES"),
        column("expires_at", "timestamp with time zone", "NO"),
        column("revoked_at", "timestamp with time zone", "YES"),
        column("created_at", "timestamp with time zone", "NO"),
    ]
}

#[tokio::test]
async fn up_down_up_leaves_the_share_links_table() {
    // Arrange
    let harness = Harness::new().await;
    let migrator = harness.db.migrator().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    revert_share(&migrator).await;
    let exists_after_down = exists(&mut conn).await;
    Migrator::run_pending(&migrator)
        .await
        .expect("re-apply the share migration");
    let exists_after_up = exists(&mut conn).await;
    let columns = columns(&mut conn).await;

    // Assert
    assert!(!exists_after_down);
    assert!(exists_after_up);
    assert_eq!(columns, expected_columns());
    conn.close().await.expect("close fixture connection");
    drop(migrator);
    harness.finish().await;
}

#[tokio::test]
async fn table_has_two_indexes_a_unique_token_hash_and_the_composite_foreign_key() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    let indexes = index_names(&mut conn).await;
    let uniques = constraint_definitions(&mut conn, "u").await;
    let foreign_keys = constraint_definitions(&mut conn, "f").await;

    // Assert
    assert_eq!(
        indexes,
        [
            "share_links_account_id_idx",
            "share_links_node_id_idx",
            "share_links_pkey",
            "share_links_token_hash_key",
        ]
    );
    assert_eq!(uniques, ["UNIQUE (token_hash)"]);
    assert_eq!(
        foreign_keys,
        [
            "FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE RESTRICT",
            "FOREIGN KEY (node_id, account_id) REFERENCES nodes(id, account_id) ON DELETE CASCADE",
        ]
    );
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn a_second_link_with_the_same_token_hash_is_refused() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let owner = Uuid::new_v4();
    let file = account_with_file(&mut conn, owner).await;
    link(&mut conn, owner, file, 1).await.expect("first link");

    // Act
    let duplicate = link(&mut conn, owner, file, 1).await;
    let other = link(&mut conn, owner, file, 2).await;

    // Assert
    let error = duplicate.expect_err("duplicate token hash");
    assert_eq!(sqlstate(&error), UNIQUE_VIOLATION);
    assert!(other.is_ok());
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn a_token_hash_that_is_not_32_bytes_is_refused() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let owner = Uuid::new_v4();
    let file = account_with_file(&mut conn, owner).await;

    // Act
    let short = sqlx::query(
        "INSERT INTO share_links (id, account_id, node_id, token_hash, expires_at, created_at) \
         VALUES ($1, $2, $3, '\\x00', 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(Uuid::new_v4())
    .bind(owner)
    .bind(file)
    .execute(&mut conn)
    .await;

    // Assert
    assert_eq!(sqlstate(&short.expect_err("short hash")), CHECK_VIOLATION);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn a_link_cannot_name_the_node_of_another_account() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let file_of_a = account_with_file(&mut conn, a).await;
    account_with_file(&mut conn, b).await;

    // Act
    let crossed = link(&mut conn, b, file_of_a, 1).await;

    // Assert
    let error = crossed.expect_err("composite foreign key");
    assert_eq!(sqlstate(&error), FOREIGN_KEY_VIOLATION);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn row_level_security_is_enabled_and_not_forced() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    let flags: (bool, bool) = sqlx::query_as(
        "SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE oid = 'public.share_links'::regclass",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read the security flags");

    // Assert
    assert_eq!(flags, (true, false));
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn runtime_role_has_dml_privileges_only() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let privileges = [
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
        "TRUNCATE",
        "REFERENCES",
        "TRIGGER",
    ];

    // Act
    let mut granted = Vec::new();
    for privilege in privileges {
        let has: bool =
            sqlx::query_scalar("SELECT has_table_privilege('sunnycloud', 'share_links', $1)")
                .bind(privilege)
                .fetch_one(&mut conn)
                .await
                .expect("check privilege");
        if has {
            granted.push(privilege);
        }
    }

    // Assert
    assert_eq!(granted, ["SELECT", "INSERT", "UPDATE", "DELETE"]);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}
