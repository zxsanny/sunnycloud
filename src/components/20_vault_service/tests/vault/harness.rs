//! Component test harness on a real PostgreSQL 18: a clone of the migrated template of `sc_testdb`. The component
//! connects as `sunnycloud`; fixtures and probes run as `sunnycloud_migrator`.

use sc_db::Db;
pub use sc_testdb::{API, MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use uuid::Uuid;

pub struct Harness {
    pub db: TestDb,
    pub api: Db,
}

impl Harness {
    pub async fn new() -> Harness {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        Harness { db, api }
    }

    /// Fixtures and probes run as the schema owner.
    pub async fn fixture_conn(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Drops the pool and the database; a connection still open afterwards is a leak of the test.
    pub async fn finish(self) {
        let Harness { db, api } = self;
        drop(api);
        assert_eq!(db.finish().await, 0, "connections left open");
    }
}

pub async fn account(conn: &mut PgConnection, id: Uuid) {
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(id)
    .bind(START.as_unix_micros())
    .execute(conn)
    .await
    .expect("insert account");
}

/// An account with its root folder; returns the root id.
pub async fn account_with_root(conn: &mut PgConnection, id: Uuid) -> Uuid {
    account(conn, id).await;
    let root = Uuid::new_v4();
    insert_node(conn, id, root, None, "folder", "").await;
    root
}

/// A live node of `kind` (`folder` or `file`, a file without a revision) under `parent`; returns its id.
pub async fn node(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Uuid,
    kind: &str,
    name: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    insert_node(conn, account, id, Some(parent), kind, name).await;
    id
}

async fn insert_node(
    conn: &mut PgConnection,
    account: Uuid,
    id: Uuid,
    parent: Option<Uuid>,
    kind: &str,
    name: &str,
) {
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $5, 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .bind(parent)
    .bind(kind)
    .bind(name)
    .execute(conn)
    .await
    .expect("insert node");
}

/// Marks `node` deleted, as a tombstone left by 02's delete.
pub async fn tombstone(conn: &mut PgConnection, node: Uuid) {
    sqlx::query("UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = $1")
        .bind(node)
        .execute(conn)
        .await
        .expect("tombstone node");
}

/// An account with its root folder and one empty folder under it; returns the folder id.
pub async fn account_with_folder(conn: &mut PgConnection, id: Uuid) -> Uuid {
    let root = account_with_root(conn, id).await;
    node(conn, id, root, "folder", "vault").await
}

/// A vault of `account` rooted at `root`; returns the vault id.
pub async fn vault(
    conn: &mut PgConnection,
    account: Uuid,
    root: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO vaults (id, account_id, root_node_id, header, created_at, updated_at) \
         VALUES ($1, $2, $3, '\\x0102'::bytea, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .bind(root)
    .execute(conn)
    .await
    .map(|_| id)
}

pub fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
        .unwrap_or_else(|| format!("not a database error: {error}"))
}
