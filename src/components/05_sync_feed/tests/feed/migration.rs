//! AC-1: the feed migration applies, reverts and re-applies as the migrator role after the tree's `nodes`
//! migration, creates the table of data_model 7.1 and adds only `nodes.seq` and `nodes_feed_idx` to `nodes`.

use sc_db::{Db, Migrator};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::support::{Fixture, account, account_with_feed, feed_row, root_node, sqlstate};

const TREE_VERSION: i64 = 20_261_106_130_000;
const FEED_VERSION: i64 = 20_261_106_150_000;
const FOREIGN_KEY_REFUSALS: [&str; 2] = ["23503", "23001"];

type Column = (String, String, String, Option<String>);

fn column(name: &str, data_type: &str, nullable: &str, default: Option<&str>) -> Column {
    (
        name.to_owned(),
        data_type.to_owned(),
        nullable.to_owned(),
        default.map(str::to_owned),
    )
}

/// Reverts the feed migration and everything applied after it.
async fn revert_feed(migrator: &Db) {
    let status = Migrator::status(migrator).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= FEED_VERSION)
        .count();
    Migrator::revert(migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the feed migration");
}

async fn columns(conn: &mut PgConnection, table: &str) -> Vec<Column> {
    sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text, column_default::text \
         FROM information_schema.columns WHERE table_schema = 'public' AND table_name = $1 \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(conn)
    .await
    .expect("list columns")
}

async fn indexes(conn: &mut PgConnection, table: &str) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT indexname::text, indexdef FROM pg_indexes \
         WHERE schemaname = 'public' AND tablename = $1 ORDER BY indexname",
    )
    .bind(table)
    .fetch_all(conn)
    .await
    .expect("list indexes")
}

async fn table_exists(conn: &mut PgConnection, table: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("public.{table}"))
        .fetch_one(conn)
        .await
        .expect("look up table")
}

fn feed_columns() -> Vec<Column> {
    vec![
        column("account_id", "uuid", "NO", None),
        column("epoch", "bigint", "NO", None),
        column("seq", "bigint", "NO", Some("0")),
        column("purge_horizon", "bigint", "NO", Some("0")),
    ]
}

#[tokio::test]
async fn up_down_up_adds_and_removes_exactly_the_feed_objects() {
    // Arrange
    let fixture = Fixture::new().await;
    let migrator = fixture.db.migrator().await;
    let mut conn = fixture.owner_conn().await;
    let nodes_columns_with_feed = columns(&mut conn, "nodes").await;
    let nodes_indexes_with_feed = indexes(&mut conn, "nodes").await;

    // Act
    revert_feed(&migrator).await;
    let feed_after_down = table_exists(&mut conn, "account_feeds").await;
    let nodes_columns_after_down = columns(&mut conn, "nodes").await;
    let nodes_indexes_after_down = indexes(&mut conn, "nodes").await;
    Migrator::run_pending(&migrator)
        .await
        .expect("re-apply the feed migration");
    let feed_columns_after_up = columns(&mut conn, "account_feeds").await;
    let nodes_columns_after_up = columns(&mut conn, "nodes").await;
    let nodes_indexes_after_up = indexes(&mut conn, "nodes").await;

    // Assert
    assert!(!feed_after_down);
    let without =
        |all: &[Column]| -> Vec<Column> { all.iter().filter(|c| c.0 != "seq").cloned().collect() };
    assert_eq!(nodes_columns_after_down, without(&nodes_columns_with_feed));
    assert!(
        nodes_columns_with_feed.contains(&column("seq", "bigint", "NO", Some("0"))),
        "{nodes_columns_with_feed:?}"
    );
    let feed_index = |all: &[(String, String)]| -> Vec<(String, String)> {
        all.iter()
            .filter(|index| index.0 == "nodes_feed_idx")
            .cloned()
            .collect()
    };
    assert_eq!(
        feed_index(&nodes_indexes_with_feed),
        [(
            "nodes_feed_idx".to_owned(),
            "CREATE INDEX nodes_feed_idx ON public.nodes USING btree (account_id, seq)".to_owned()
        )]
    );
    assert!(feed_index(&nodes_indexes_after_down).is_empty());
    assert_eq!(
        nodes_indexes_after_down.len() + 1,
        nodes_indexes_with_feed.len()
    );
    assert_eq!(feed_columns_after_up, feed_columns());
    assert_eq!(nodes_columns_after_up, nodes_columns_with_feed);
    assert_eq!(nodes_indexes_after_up, nodes_indexes_with_feed);
    conn.close().await.expect("close owner connection");
    drop(migrator);
    fixture.finish().await;
}

#[tokio::test]
async fn the_feed_migration_sorts_after_the_tree_migration_and_applies_after_it() {
    // Arrange
    let fixture = Fixture::new().await;
    let migrator = fixture.db.migrator().await;

    // Act
    let applied = Migrator::status(&migrator)
        .await
        .expect("migration status")
        .applied;

    // Assert
    let position = |version: i64| applied.iter().position(|v| *v == version);
    let tree = position(TREE_VERSION).expect("tree migration applied");
    let feed = position(FEED_VERSION).expect("feed migration applied");
    assert!(tree < feed, "{applied:?}");
    drop(migrator);
    fixture.finish().await;
}

#[tokio::test]
async fn account_feeds_has_the_columns_and_the_key_of_the_data_model() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let found = columns(&mut conn, "account_feeds").await;
    let found_indexes = indexes(&mut conn, "account_feeds").await;

    // Assert
    assert_eq!(found, feed_columns());
    assert_eq!(
        found_indexes,
        [(
            "account_feeds_pkey".to_owned(),
            "CREATE UNIQUE INDEX account_feeds_pkey ON public.account_feeds USING btree (account_id)"
                .to_owned()
        )]
    );
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn counters_default_to_zero_and_the_epoch_must_be_given() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let id = Uuid::new_v4();
    account(&mut conn, id).await;

    // Act
    let without_epoch = sqlx::query("INSERT INTO account_feeds (account_id) VALUES ($1)")
        .bind(id)
        .execute(&mut conn)
        .await;
    feed_row(&mut conn, id, 1_794_000_000)
        .await
        .expect("insert with defaults");
    let row: (i64, i64, i64) =
        sqlx::query_as("SELECT epoch, seq, purge_horizon FROM account_feeds WHERE account_id = $1")
            .bind(id)
            .fetch_one(&mut conn)
            .await
            .expect("read the row");

    // Assert
    assert_eq!(
        sqlstate(&without_epoch.expect_err("epoch is required")),
        "23502"
    );
    assert_eq!(row, (1_794_000_000, 0, 0));
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn a_node_starts_with_seq_zero() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let owner = Uuid::new_v4();
    account(&mut conn, owner).await;

    // Act
    let node = root_node(&mut conn, owner).await;
    let seq: i64 = sqlx::query_scalar("SELECT seq FROM nodes WHERE id = $1")
        .bind(node)
        .fetch_one(&mut conn)
        .await
        .expect("read seq");

    // Assert
    assert_eq!(seq, 0);
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn feed_row_of_an_unknown_account_or_of_a_deleted_account_is_refused() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let owner = Uuid::new_v4();
    account_with_feed(&mut conn, owner, 1).await;

    // Act
    let unknown = feed_row(&mut conn, Uuid::new_v4(), 1).await;
    let duplicate = feed_row(&mut conn, owner, 1).await;
    let delete_owner = sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(owner)
        .execute(&mut conn)
        .await;

    // Assert
    for error in [
        unknown.expect_err("unknown account"),
        delete_owner.expect_err("account with a feed row"),
    ] {
        assert!(FOREIGN_KEY_REFUSALS.contains(&sqlstate(&error).as_str()));
    }
    assert_eq!(
        sqlstate(&duplicate.expect_err("one row per account")),
        "23505"
    );
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}

#[tokio::test]
async fn runtime_role_has_dml_privileges_only() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
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
            sqlx::query_scalar("SELECT has_table_privilege('sunnycloud', 'account_feeds', $1)")
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
    conn.close().await.expect("close owner connection");
    fixture.finish().await;
}
