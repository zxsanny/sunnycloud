//! One database per test, a clone of the migrated `sc_testdb` template. The component side connects as
//! `sunnycloud`, fixtures and probes as `sunnycloud_migrator`.

use sc_db::Db;
pub use sc_testdb::{API, MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use uuid::Uuid;

pub struct Fixture {
    pub db: TestDb,
    pub api: Db,
}

impl Fixture {
    pub async fn new() -> Fixture {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        Fixture { db, api }
    }

    pub async fn owner_conn(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Drops the pool and the database; a connection still open afterwards is a leak of the test.
    pub async fn finish(self) {
        let Fixture { db, api } = self;
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

pub async fn blob(conn: &mut PgConnection, id: Uuid, account: Uuid) {
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .execute(conn)
    .await
    .expect("insert blob");
}

/// A node row; `name_key` is the lower-cased name, which is all these tests need of the application's fold.
pub async fn node(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Option<Uuid>,
    kind: &str,
    name: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    node_row(conn, id, account, parent, kind, name, &name.to_lowercase()).await?;
    Ok(id)
}

pub async fn node_row(
    conn: &mut PgConnection,
    id: Uuid,
    account: Uuid,
    parent: Option<Uuid>,
    kind: &str,
    name: &str,
    name_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .bind(parent)
    .bind(kind)
    .bind(name)
    .bind(name_key)
    .execute(conn)
    .await
    .map(drop)
}

pub struct RevisionRow<'a> {
    pub id: Uuid,
    pub account: Uuid,
    pub node: Uuid,
    pub blob: Uuid,
    pub size: i64,
    pub content_hash: &'a [u8],
    pub blocklist: &'a [u8],
    pub client_meta: Option<&'a str>,
}

impl RevisionRow<'static> {
    pub fn new(account: Uuid, node: Uuid, blob: Uuid) -> Self {
        RevisionRow {
            id: Uuid::new_v4(),
            account,
            node,
            blob,
            size: 1,
            content_hash: &[7; 32],
            blocklist: &[9; 32],
            client_meta: None,
        }
    }
}

pub async fn revision(conn: &mut PgConnection, row: &RevisionRow<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, client_meta, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, 'epoch'::timestamptz)",
    )
    .bind(row.id)
    .bind(row.account)
    .bind(row.node)
    .bind(row.blob)
    .bind(row.size)
    .bind(row.content_hash)
    .bind(row.blocklist)
    .bind(row.client_meta)
    .execute(conn)
    .await
    .map(drop)
}

/// An account with its root folder; returns the root id.
pub async fn account_with_root(conn: &mut PgConnection, id: Uuid) -> Uuid {
    account(conn, id).await;
    node(conn, id, None, "folder", "")
        .await
        .expect("insert root")
}

/// A file node of `parent` with one revision of 1 byte over a fresh blob, the revision current, counted in usage
/// as `publish` would when the account has a quota row.
pub async fn file_with_revision(
    conn: &mut PgConnection,
    account: Uuid,
    parent: Uuid,
    name: &str,
) -> (Uuid, Uuid, Uuid) {
    let (blob_id, revision_id) = (Uuid::new_v4(), Uuid::new_v4());
    blob(conn, blob_id, account).await;
    let file = node(conn, account, Some(parent), "file", name)
        .await
        .expect("insert file");
    revision(
        conn,
        &RevisionRow {
            id: revision_id,
            ..RevisionRow::new(account, file, blob_id)
        },
    )
    .await
    .expect("insert revision");
    sqlx::query("UPDATE nodes SET current_revision_id = $1 WHERE id = $2")
        .bind(revision_id)
        .bind(file)
        .execute(&mut *conn)
        .await
        .expect("point the file at its revision");
    sqlx::query("UPDATE quota SET used_bytes = used_bytes + 1 WHERE account_id = $1")
        .bind(account)
        .execute(conn)
        .await
        .expect("count the file in usage when the account has a quota row");
    (file, revision_id, blob_id)
}

/// SQLSTATE and constraint name of a refused statement.
pub fn refusal<T: std::fmt::Debug>(result: Result<T, sqlx::Error>) -> (String, String) {
    match result {
        Err(sqlx::Error::Database(error)) => (
            error
                .code()
                .map(|code| code.to_string())
                .unwrap_or_default(),
            error.constraint().unwrap_or_default().to_owned(),
        ),
        other => panic!("expected a database error, got {other:?}"),
    }
}
