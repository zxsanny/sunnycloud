//! One database per test, a clone of the migrated `sc_testdb` template. The component side connects as
//! `sunnycloud`, fixtures and probes as `sunnycloud_migrator`.

use sc_db::Db;
pub use sc_testdb::{API, MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use uuid::Uuid;

const BLOCK: i64 = 2 * 1024 * 1024;

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

pub fn at() -> i64 {
    START.as_unix_micros()
}

pub async fn account(conn: &mut PgConnection, id: Uuid) {
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(id)
    .bind(at())
    .execute(conn)
    .await
    .expect("insert account");
}

pub async fn blob(conn: &mut PgConnection, id: Uuid, account: Uuid) {
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'uploading', '\\x00', 1, 'epoch'::timestamptz + $3::bigint * interval '1 microsecond')",
    )
    .bind(id)
    .bind(account)
    .bind(at())
    .execute(conn)
    .await
    .expect("insert blob");
}

/// The columns of an `upload_sessions` row; the defaults describe a consistent row of `declared_size` bytes.
#[derive(Clone)]
pub struct Session {
    pub id: Uuid,
    pub account: Uuid,
    pub blob: Uuid,
    pub client_key: String,
    pub declared_size: i64,
    pub block_count: i32,
    pub reserved_bytes: i64,
    pub received_len: usize,
    pub hashes_len: usize,
}

impl Session {
    pub fn new(account: Uuid, blob: Uuid, client_key: &str, declared_size: i64) -> Session {
        let block_count = i32::try_from((declared_size + BLOCK - 1) / BLOCK).expect("block count");
        Session {
            id: Uuid::new_v4(),
            account,
            blob,
            client_key: client_key.to_owned(),
            declared_size,
            block_count,
            reserved_bytes: declared_size,
            received_len: usize::try_from(block_count)
                .expect("block count")
                .div_ceil(8),
            hashes_len: usize::try_from(block_count).expect("block count") * 32,
        }
    }
}

const INSERT_SESSION: &str = "INSERT INTO upload_sessions (id, account_id, blob_id, client_key, declared_size, \
     block_count, reserved_bytes, received, block_hashes, created_at, last_checkpoint_at, expires_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, \
     'epoch'::timestamptz, 'epoch'::timestamptz, 'epoch'::timestamptz)";

pub async fn insert_session(conn: &mut PgConnection, row: &Session) -> Result<(), sqlx::Error> {
    sqlx::query(INSERT_SESSION)
        .bind(row.id)
        .bind(row.account)
        .bind(row.blob)
        .bind(&row.client_key)
        .bind(row.declared_size)
        .bind(row.block_count)
        .bind(row.reserved_bytes)
        .bind(vec![0u8; row.received_len])
        .bind(vec![0u8; row.hashes_len])
        .execute(conn)
        .await
        .map(drop)
}

/// A session with its own blob, inserted as the owner.
pub async fn session(
    conn: &mut PgConnection,
    account: Uuid,
    client_key: &str,
    declared_size: i64,
) -> Uuid {
    let blob_id = Uuid::new_v4();
    blob(conn, blob_id, account).await;
    let row = Session::new(account, blob_id, client_key, declared_size);
    insert_session(conn, &row).await.expect("insert session");
    row.id
}

pub async fn receipt(
    conn: &mut PgConnection,
    account: Uuid,
    commit_id: Uuid,
    hash_len: usize,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
         VALUES ($1, $2, $3, '[]'::jsonb, 'epoch'::timestamptz)",
    )
    .bind(account)
    .bind(commit_id)
    .bind(vec![7u8; hash_len])
    .execute(conn)
    .await
    .map(drop)
}

/// SQLSTATE and constraint name of a refused statement.
pub fn refusal(result: Result<(), sqlx::Error>) -> (String, String) {
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
