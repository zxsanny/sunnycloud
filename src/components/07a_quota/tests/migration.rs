//! AC-1: the quota migration applies, reverts and re-applies as the migrator role, creates the table of
//! data_model 8.2 and grants the runtime role DML only.

mod support;

use sc_db::{Db, Migrator};
use sqlx::{Connection, PgConnection};
use support::{Harness, account, insert_quota};
use uuid::Uuid;

const QUOTA_VERSION: i64 = 20_261_106_110_000;
const PERMISSION_DENIED: &str = "42501";
const CHECK_VIOLATION: &str = "23514";

type Column = (String, String, String, Option<String>);

/// Reverts the quota migration and everything applied after it.
async fn revert_quota(migrator: &Db) {
    let status = Migrator::status(migrator).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= QUOTA_VERSION)
        .count();
    Migrator::revert(migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the quota migration");
}

async fn quota_columns(conn: &mut PgConnection) -> Vec<Column> {
    sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text, column_default::text \
         FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'quota' \
         ORDER BY ordinal_position",
    )
    .fetch_all(conn)
    .await
    .expect("list quota columns")
}

async fn quota_exists(conn: &mut PgConnection) -> bool {
    sqlx::query_scalar("SELECT to_regclass('public.quota') IS NOT NULL")
        .fetch_one(conn)
        .await
        .expect("look up quota")
}

fn expected_columns() -> Vec<Column> {
    let column = |name: &str, nullable: &str, default: Option<&str>| {
        (
            name.to_owned(),
            if name == "account_id" {
                "uuid"
            } else {
                "bigint"
            }
            .to_owned(),
            nullable.to_owned(),
            default.map(str::to_owned),
        )
    };
    vec![
        column("account_id", "NO", None),
        column("limit_bytes", "YES", None),
        column("used_bytes", "NO", Some("0")),
        column("reserved_bytes", "NO", Some("0")),
    ]
}

fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
        .unwrap_or_else(|| format!("not a database error: {error}"))
}

#[tokio::test]
async fn up_down_up_leaves_the_quota_table() {
    // Arrange
    let harness = Harness::new().await;
    let migrator = harness.db.migrator().await;
    let mut conn = harness.fixture_conn().await;

    // Act
    revert_quota(&migrator).await;
    let exists_after_down = quota_exists(&mut conn).await;
    Migrator::run_pending(&migrator)
        .await
        .expect("re-apply the quota migration");
    let exists_after_up = quota_exists(&mut conn).await;
    let columns = quota_columns(&mut conn).await;

    // Assert
    assert!(!exists_after_down);
    assert!(exists_after_up);
    assert_eq!(columns, expected_columns());
    conn.close().await.expect("close fixture connection");
    drop(migrator);
    harness.finish().await;
}

#[tokio::test]
async fn used_or_reserved_below_zero_violates_the_check() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    account(&mut conn, a).await;
    account(&mut conn, b).await;

    // Act
    let negative_used = insert_quota(&mut conn, a, None, -1, 0).await;
    let negative_reserved = insert_quota(&mut conn, b, None, 0, -1).await;

    // Assert
    for result in [negative_used, negative_reserved] {
        let error = result.expect_err("a negative counter is refused");
        assert_eq!(sqlstate(&error), CHECK_VIOLATION);
    }
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn counters_default_to_zero_and_the_limit_to_null() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let id = Uuid::new_v4();
    account(&mut conn, id).await;

    // Act
    sqlx::query("INSERT INTO quota (account_id) VALUES ($1)")
        .bind(id)
        .execute(&mut conn)
        .await
        .expect("insert with defaults");
    let row: (Option<i64>, i64, i64) = sqlx::query_as(
        "SELECT limit_bytes, used_bytes, reserved_bytes FROM quota WHERE account_id = $1",
    )
    .bind(id)
    .fetch_one(&mut conn)
    .await
    .expect("read the row");

    // Assert
    assert_eq!(row, (None, 0, 0));
    conn.close().await.expect("close fixture connection");
    harness.finish().await;
}

#[tokio::test]
async fn quota_row_of_an_unknown_account_or_of_a_deleted_account_is_refused() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let owner = Uuid::new_v4();
    account(&mut conn, owner).await;
    insert_quota(&mut conn, owner, None, 0, 0)
        .await
        .expect("insert quota row");

    // Act
    let unknown = insert_quota(&mut conn, Uuid::new_v4(), None, 0, 0).await;
    let delete_owner = sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(owner)
        .execute(&mut conn)
        .await;

    // Assert
    for error in [
        unknown.expect_err("unknown account"),
        delete_owner.expect_err("account with a quota row"),
    ] {
        assert!(["23503", "23001"].contains(&sqlstate(&error).as_str()));
    }
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
        let has: bool = sqlx::query_scalar("SELECT has_table_privilege('sunnycloud', 'quota', $1)")
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

#[tokio::test]
async fn runtime_role_cannot_alter_or_truncate_the_table() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.db.conn_as(support::API).await;

    // Act
    let alter = sqlx::raw_sql("ALTER TABLE quota ADD COLUMN extra bigint")
        .execute(&mut conn)
        .await;
    let truncate = sqlx::raw_sql("TRUNCATE quota").execute(&mut conn).await;

    // Assert
    for result in [alter, truncate] {
        let error = result.expect_err("DDL is refused");
        assert_eq!(sqlstate(&error), PERMISSION_DENIED);
    }
    conn.close().await.expect("close api connection");
    harness.finish().await;
}
