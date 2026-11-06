//! Component test harness on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL): a clone of the migrated
//! template of `sc_testdb`. The component connects as `sunnycloud`; fixtures and probes run as the schema owner
//! `sunnycloud_migrator`.

#![allow(dead_code, unused_imports)]

pub mod settings;

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

    pub async fn fixture_conn(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Releases the component's connections and drops the database; returns the connections still open.
    pub async fn finish(self) -> i64 {
        let Harness { db, api } = self;
        drop(api);
        db.finish().await
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

/// An account with its quota row.
pub async fn account_with_quota(
    conn: &mut PgConnection,
    id: Uuid,
    limit: Option<i64>,
    used: i64,
    reserved: i64,
) {
    account(conn, id).await;
    insert_quota(conn, id, limit, used, reserved)
        .await
        .expect("insert quota row");
}

pub async fn insert_quota(
    conn: &mut PgConnection,
    id: Uuid,
    limit: Option<i64>,
    used: i64,
    reserved: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO quota (account_id, limit_bytes, used_bytes, reserved_bytes) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(limit)
    .bind(used)
    .bind(reserved)
    .execute(conn)
    .await
    .map(drop)
}
