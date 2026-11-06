//! Component test harness on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL): a clone of the migrated
//! template of `sc_testdb`. The component connects as `sunnycloud`; fixtures and probes run as the schema owner
//! `sunnycloud_migrator`.

#![allow(dead_code, unused_imports)]

pub mod settings;

use std::time::Duration;

use sc_db::{Db, DbError, DbRole, DbTx, Scope};
pub use sc_testdb::{API, MIGRATOR, START, TestDb, clock_at_start};
use sqlx::PgConnection;
use sunnycloud_quota::{PgQuotaLedger, QuotaConfig};
use uuid::Uuid;

pub const MIB: i64 = 1 << 20;
pub const GIB: i64 = 1 << 30;
pub const TIB: i64 = 1 << 40;

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

    /// The API role with a larger pool, for tests that hold many transactions open at once.
    pub async fn api_with_pool(&self, max: u32) -> Db {
        let mut config = self.db.config();
        config.db_pool_max = max;
        config.db_acquire_timeout = Duration::from_secs(30);
        Db::connect(&config, DbRole::Api)
            .await
            .expect("connect as the API role")
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

pub fn ledger(default_quota_bytes: i64) -> PgQuotaLedger {
    PgQuotaLedger::new(QuotaConfig {
        default_quota_bytes,
    })
}

/// Runs `f` in one committed transaction scoped to `account`.
pub async fn in_tx<T>(db: &Db, account: Uuid, f: impl AsyncFnOnce(&mut DbTx) -> T) -> T {
    db.run(Scope::Account(account), async |tx| {
        Ok::<_, DbError>(f(tx).await)
    })
    .await
    .expect("transaction")
}

/// `(limit_bytes, used_bytes, reserved_bytes)` as the schema owner sees them.
pub async fn quota_row(conn: &mut PgConnection, id: Uuid) -> (Option<i64>, i64, i64) {
    sqlx::query_as(
        "SELECT limit_bytes, used_bytes, reserved_bytes FROM quota WHERE account_id = $1",
    )
    .bind(id)
    .fetch_one(conn)
    .await
    .expect("read quota row")
}
