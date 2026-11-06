//! AC-1: the tree migration applies, reverts and re-applies as the migrator role, creates the tables of
//! data_model 5.1 and 5.2 with their indexes and constraints, and grants the runtime role DML only.

use sc_db::{Db, Migrator};
use sqlx::{Connection, PgConnection};

use crate::support::{API, Fixture, refusal};

const TREE_VERSION: i64 = 20_261_106_130_000;
const PERMISSION_DENIED: &str = "42501";

const NODE_INDEXES: [&str; 7] = [
    "nodes_cascade_idx",
    "nodes_id_account_id_key",
    "nodes_parent_idx",
    "nodes_parent_name_key",
    "nodes_pkey",
    "nodes_root_key",
    "nodes_tombstone_idx",
];
const REVISION_INDEXES: [&str; 6] = [
    "revisions_account_id_content_hash_idx",
    "revisions_blob_id_key",
    "revisions_client_device_idx",
    "revisions_id_account_id_key",
    "revisions_node_id_created_at_idx",
    "revisions_pkey",
];

/// Reverts the tree migration and everything applied after it.
async fn revert_tree(migrator: &Db) {
    let status = Migrator::status(migrator).await.expect("migration status");
    let steps = status
        .applied
        .iter()
        .filter(|version| **version >= TREE_VERSION)
        .count();
    Migrator::revert(migrator, u32::try_from(steps).expect("step count"))
        .await
        .expect("revert the tree migration");
}

async fn exists(conn: &mut PgConnection, table: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("public.{table}"))
        .fetch_one(conn)
        .await
        .expect("look up the table")
}

async fn names(conn: &mut PgConnection, sql: &'static str, table: &str) -> Vec<String> {
    sqlx::query_scalar(sql)
        .bind(table)
        .fetch_all(conn)
        .await
        .expect("list names")
}

async fn index_names(conn: &mut PgConnection, table: &str) -> Vec<String> {
    names(
        conn,
        "SELECT indexname::text FROM pg_indexes WHERE schemaname = 'public' AND tablename = $1 ORDER BY 1",
        table,
    )
    .await
}

async fn constraint_names(conn: &mut PgConnection, table: &str) -> Vec<String> {
    names(
        conn,
        "SELECT conname::text FROM pg_constraint WHERE conrelid = ('public.' || $1)::regclass AND contype <> 'n' ORDER BY 1",
        table,
    )
    .await
}

async fn index_definition(conn: &mut PgConnection, index: &str) -> String {
    sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE indexname = $1")
        .bind(index)
        .fetch_one(conn)
        .await
        .expect("read the index definition")
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    all.sort();
    all
}

#[tokio::test]
async fn up_down_up_leaves_both_tables() {
    // Arrange
    let fixture = Fixture::new().await;
    let migrator = fixture.db.migrator().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    revert_tree(&migrator).await;
    let after_down = (
        exists(&mut conn, "nodes").await,
        exists(&mut conn, "revisions").await,
    );
    Migrator::run_pending(&migrator)
        .await
        .expect("re-apply the tree migration");
    let after_up = (
        exists(&mut conn, "nodes").await,
        exists(&mut conn, "revisions").await,
    );

    // Assert
    assert_eq!(after_down, (false, false));
    assert_eq!(after_up, (true, true));
    conn.close().await.expect("close fixture connection");
    drop(migrator);
    fixture.finish().await;
}

#[tokio::test]
async fn every_index_of_the_specification_exists() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let nodes = index_names(&mut conn, "nodes").await;
    let revisions = index_names(&mut conn, "revisions").await;
    let defs = [
        index_definition(&mut conn, "nodes_parent_name_key").await,
        index_definition(&mut conn, "nodes_root_key").await,
        index_definition(&mut conn, "nodes_tombstone_idx").await,
        index_definition(&mut conn, "nodes_cascade_idx").await,
        index_definition(&mut conn, "revisions_client_device_idx").await,
    ];

    // Assert
    assert_eq!(nodes, sorted(&NODE_INDEXES));
    assert_eq!(revisions, sorted(&REVISION_INDEXES));
    assert!(defs[0].starts_with("CREATE UNIQUE INDEX"));
    assert!(defs[0].contains("(parent_id, name_key)"));
    assert!(defs[0].contains("WHERE (deleted_at IS NULL)"));
    assert!(defs[1].starts_with("CREATE UNIQUE INDEX"));
    assert!(defs[1].contains("WHERE (parent_id IS NULL)"));
    assert!(defs[2].contains("WHERE (deleted_at IS NOT NULL)"));
    assert!(defs[3].contains("WHERE cascade_pending"));
    assert!(defs[4].contains("((client_meta ->> 'device'::text))"));
    assert!(defs[4].contains("WHERE (client_meta IS NOT NULL)"));
    conn.close().await.expect("close fixture connection");
    fixture.finish().await;
}

#[tokio::test]
async fn every_constraint_of_the_specification_exists() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;
    let expected_nodes = [
        "nodes_account_id_fkey",
        "nodes_current_revision_id_account_id_fkey",
        "nodes_file_parent_check",
        "nodes_folder_revision_check",
        "nodes_id_account_id_key",
        "nodes_kind_check",
        "nodes_name_check",
        "nodes_name_key_size_check",
        "nodes_name_size_check",
        "nodes_parent_id_account_id_fkey",
        "nodes_pkey",
    ];
    let expected_revisions = [
        "revisions_blob_id_account_id_fkey",
        "revisions_blob_id_key",
        "revisions_blocklist_check",
        "revisions_client_meta_check",
        "revisions_content_hash_check",
        "revisions_id_account_id_key",
        "revisions_node_id_account_id_fkey",
        "revisions_pkey",
        "revisions_size_check",
    ];

    // Act
    let nodes = constraint_names(&mut conn, "nodes").await;
    let revisions = constraint_names(&mut conn, "revisions").await;
    let deferred: Vec<(String, bool, bool, String)> = sqlx::query_as(
        "SELECT conname::text, condeferrable, condeferred, confdeltype::text FROM pg_constraint \
         WHERE conname IN ('nodes_current_revision_id_account_id_fkey', 'revisions_blob_id_account_id_fkey') \
         ORDER BY conname",
    )
    .fetch_all(&mut conn)
    .await
    .expect("read the foreign key modes");

    // Assert
    assert_eq!(nodes, sorted(&expected_nodes));
    assert_eq!(revisions, sorted(&expected_revisions));
    assert_eq!(
        deferred,
        vec![
            (
                "nodes_current_revision_id_account_id_fkey".to_owned(),
                true,
                true,
                "n".to_owned()
            ),
            (
                "revisions_blob_id_account_id_fkey".to_owned(),
                false,
                false,
                "r".to_owned()
            ),
        ]
    );
    conn.close().await.expect("close fixture connection");
    fixture.finish().await;
}

#[tokio::test]
async fn row_level_security_is_enabled_without_force_and_the_blocklist_is_stored_out_of_line() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.owner_conn().await;

    // Act
    let rls: Vec<(String, bool, bool)> = sqlx::query_as(
        "SELECT relname::text, relrowsecurity, relforcerowsecurity FROM pg_class \
         WHERE oid IN ('public.nodes'::regclass, 'public.revisions'::regclass) ORDER BY relname",
    )
    .fetch_all(&mut conn)
    .await
    .expect("read the security flags");
    let storage: String = sqlx::query_scalar(
        "SELECT attstorage::text FROM pg_attribute WHERE attrelid = 'public.revisions'::regclass AND attname = 'blocklist'",
    )
    .fetch_one(&mut conn)
    .await
    .expect("read the storage mode");
    let options: Vec<String> = sqlx::query_scalar(
        "SELECT unnest(reloptions) FROM pg_class WHERE oid = 'public.revisions'::regclass",
    )
    .fetch_all(&mut conn)
    .await
    .expect("read the table options");

    // Assert
    assert_eq!(
        rls,
        vec![
            ("nodes".to_owned(), true, false),
            ("revisions".to_owned(), true, false)
        ]
    );
    assert_eq!(storage, "e");
    assert_eq!(options, ["toast_tuple_target=256"]);
    conn.close().await.expect("close fixture connection");
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
    for table in ["nodes", "revisions"] {
        let mut held = Vec::new();
        for privilege in privileges {
            let has: bool = sqlx::query_scalar("SELECT has_table_privilege('sunnycloud', $1, $2)")
                .bind(table)
                .bind(privilege)
                .fetch_one(&mut conn)
                .await
                .expect("check privilege");
            if has {
                held.push(privilege);
            }
        }
        granted.push(held);
    }

    // Assert
    for held in granted {
        assert_eq!(held, ["SELECT", "INSERT", "UPDATE", "DELETE"]);
    }
    conn.close().await.expect("close fixture connection");
    fixture.finish().await;
}

#[tokio::test]
async fn runtime_role_cannot_alter_truncate_or_disable_security() {
    // Arrange
    let fixture = Fixture::new().await;
    let mut conn = fixture.db.conn_as(API).await;
    let statements = [
        "ALTER TABLE nodes ADD COLUMN extra bigint",
        "ALTER TABLE revisions DISABLE ROW LEVEL SECURITY",
        "TRUNCATE nodes",
        "TRUNCATE revisions",
        "DROP TABLE revisions",
        "CREATE INDEX stray_idx ON nodes (name)",
    ];

    // Act
    let mut codes = Vec::new();
    for statement in statements {
        let result = sqlx::raw_sql(statement).execute(&mut conn).await;
        codes.push(refusal(result).0);
    }

    // Assert
    assert_eq!(codes, [PERMISSION_DENIED; 6]);
    conn.close().await.expect("close api connection");
    fixture.finish().await;
}
