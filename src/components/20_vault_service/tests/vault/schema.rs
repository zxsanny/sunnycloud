//! AC-1 (table shape, no `vault_files`), AC-3 (the root belongs to the same account) and AC-4 (up, down, up) of
//! the vault migration, as the migrator role on a real PostgreSQL 18; plus the one-vault-per-account rule and the
//! flags and grants of the table.

use sc_db::{Db, Migrator};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::harness::{Harness, account_with_folder, sqlstate, vault};

const VAULT_VERSION: i64 = 20_261_112_100_000;
const UNIQUE_VIOLATION: &str = "23505";
const FOREIGN_KEY_VIOLATION: &str = "23503";

type Column = (String, String, String);

/// Reverts the vault migration and everything applied after it.
async fn revert_vault(migrator: &Db) {
    let status = Migrator::status(migrator).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= VAULT_VERSION)
        .count();
    Migrator::revert(migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the vault migration");
}

async fn table_exists(conn: &mut PgConnection, name: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("public.{name}"))
        .fetch_one(conn)
        .await
        .expect("look up the table")
}

async fn columns(conn: &mut PgConnection) -> Vec<Column> {
    sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'vaults' ORDER BY ordinal_position",
    )
    .fetch_all(conn)
    .await
    .expect("list columns")
}

async fn constraint_definitions(conn: &mut PgConnection, kind: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
         WHERE conrelid = 'public.vaults'::regclass AND contype = $1::\"char\" ORDER BY conname",
    )
    .bind(kind)
    .fetch_all(conn)
    .await
    .expect("list constraints")
}

fn expected_columns() -> Vec<Column> {
    let column =
        |name: &str, data_type: &str| (name.to_owned(), data_type.to_owned(), "NO".to_owned());
    vec![
        column("id", "uuid"),
        column("account_id", "uuid"),
        column("root_node_id", "uuid"),
        column("header", "bytea"),
        column("created_at", "timestamp with time zone"),
        column("updated_at", "timestamp with time zone"),
    ]
}

#[tokio::test]
async fn up_down_up_leaves_the_vaults_table() {
    // Arrange
    let harness = Harness::new().await;
    let migrator = harness.db.migrator().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    revert_vault(&migrator).await;
    let exists_after_down = table_exists(&mut conn, "vaults").await;
    Migrator::run_pending(&migrator)
        .await
        .expect("re-apply the vault migration");
    let exists_after_up = table_exists(&mut conn, "vaults").await;

    // Assert
    assert!(!exists_after_down);
    assert!(exists_after_up);
    assert_eq!(columns(&mut conn).await, expected_columns());
    conn.close().await.expect("close fixture connection");
    drop(migrator);
    harness.finish().await;
}

#[tokio::test]
async fn table_has_the_six_columns_the_unique_keys_and_the_composite_foreign_key() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    let columns = columns(&mut conn).await;
    let uniques = constraint_definitions(&mut conn, "u").await;
    let foreign_keys = constraint_definitions(&mut conn, "f").await;

    // Assert
    assert_eq!(columns, expected_columns());
    assert_eq!(uniques, ["UNIQUE (account_id)", "UNIQUE (root_node_id)"]);
    assert_eq!(
        foreign_keys,
        [
            "FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE RESTRICT",
            "FOREIGN KEY (root_node_id, account_id) REFERENCES nodes(id, account_id) ON DELETE CASCADE",
        ]
    );
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn no_vault_files_table_exists() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    let exists = table_exists(&mut conn, "vault_files").await;

    // Assert
    assert!(!exists);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn a_vault_cannot_name_the_folder_of_another_account_as_root() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    account_with_folder(&mut conn, a).await;
    let folder_of_b = account_with_folder(&mut conn, b).await;

    // Act
    let crossed = vault(&mut conn, a, folder_of_b).await;

    // Assert
    let error = crossed.expect_err("composite foreign key");
    assert_eq!(sqlstate(&error), FOREIGN_KEY_VIOLATION);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn a_second_vault_for_the_same_account_is_refused() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let owner = Uuid::new_v4();
    let folder = account_with_folder(&mut conn, owner).await;
    vault(&mut conn, owner, folder).await.expect("first vault");
    let other_folder = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         SELECT $1, account_id, parent_id, 'folder', 'other', 'other', 1, 'epoch'::timestamptz, 'epoch'::timestamptz \
         FROM nodes WHERE id = $2",
    )
    .bind(other_folder)
    .bind(folder)
    .execute(&mut conn)
    .await
    .expect("insert a second folder");

    // Act
    let second = vault(&mut conn, owner, other_folder).await;

    // Assert
    let error = second.expect_err("one vault per account");
    assert_eq!(sqlstate(&error), UNIQUE_VIOLATION);
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

/// Owner decision 2026-11-13 (`*_cascade_vault_on_root_purge`): purging the root folder removes the vault.
#[tokio::test]
async fn deleting_the_root_folder_of_a_vault_removes_the_vault() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let owner = Uuid::new_v4();
    let folder = account_with_folder(&mut conn, owner).await;
    vault(&mut conn, owner, folder).await.expect("vault");

    // Act
    let deleted = sqlx::query("DELETE FROM nodes WHERE id = $1")
        .bind(folder)
        .execute(&mut conn)
        .await;

    // Assert
    assert_eq!(deleted.expect("delete root").rows_affected(), 1);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM vaults WHERE account_id = $1")
        .bind(owner)
        .fetch_one(&mut conn)
        .await
        .expect("count vaults");
    assert_eq!(left, 0);
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
        "SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE oid = 'public.vaults'::regclass",
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
            sqlx::query_scalar("SELECT has_table_privilege('sunnycloud', 'vaults', $1)")
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
