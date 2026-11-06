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

pub async fn blob(
    conn: &mut PgConnection,
    id: Uuid,
    account: Uuid,
    state: &str,
    size: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, $3, $4, $5, 1, 'epoch'::timestamptz + $6::bigint * interval '1 microsecond')",
    )
    .bind(id)
    .bind(account)
    .bind(state)
    .bind(size)
    .bind(WRAPPED_KEY)
    .bind(at())
    .execute(conn)
    .await
    .map(drop)
}

/// A recognisable stand-in for wrapped key bytes: no formatted error may contain it.
pub const WRAPPED_KEY: &[u8] = b"sentinel-wrapped-key";

pub async fn gc_row(
    conn: &mut PgConnection,
    blob: Uuid,
    account: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO blob_gc (blob_id, account_id, due_at, reason) \
         VALUES ($1, $2, 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', $4)",
    )
    .bind(blob)
    .bind(account)
    .bind(at())
    .bind(reason)
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
