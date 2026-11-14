//! What the purge left, read as the schema owner: the account's rows in every table with an `account_id` column,
//! its audit rows, its state and its blob files on the volume.

use std::collections::BTreeMap;
use std::time::Duration;

use contracts::Janitor;
use sc_testdb::MIGRATOR;
use sqlx::{AssertSqlSafe, PgConnection};
use sunnycloud_admin::DeletionPurger;
use sunnycloud_blobstore::{GcSweeper, blob_path};
use uuid::Uuid;

use super::world::PurgeWorld;

/// More purger cycles than any account of the suite needs; a run past it is a stuck purge.
pub const MAX_CYCLES: usize = 400;

/// Every table of the schema with an `account_id` column (the AC-1 sweep finds a table nobody registered).
pub async fn account_tables(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND column_name = 'account_id' ORDER BY table_name",
    )
    .fetch_all(conn)
    .await
    .expect("list account tables")
}

/// Rows of `account` per table, the `accounts` row included.
pub async fn rows_of(conn: &mut PgConnection, account: Uuid) -> BTreeMap<String, i64> {
    let mut rows = BTreeMap::new();
    for table in account_tables(conn).await {
        let sql = format!("SELECT count(*) FROM {table} WHERE account_id = $1");
        let count: i64 = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(account)
            .fetch_one(&mut *conn)
            .await
            .expect("count rows");
        rows.insert(table, count);
    }
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(&mut *conn)
        .await
        .expect("count account row");
    rows.insert("accounts".to_owned(), accounts);
    rows
}

pub fn total(rows: &BTreeMap<String, i64>) -> i64 {
    rows.values().sum()
}

/// `account_deleted` audit rows about `account`.
pub async fn deleted_audits(conn: &mut PgConnection, account: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE event = 'account_deleted' AND subject_id = $1",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count audit rows")
}

/// The account's state; `None` once the row is gone.
pub async fn state(conn: &mut PgConnection, account: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT state FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_optional(conn)
        .await
        .expect("read state")
}

/// Blob files of `blobs` still on the volume, parked (`.gc`) ones included.
pub fn files_left(world: &PurgeWorld, blobs: &[Uuid]) -> usize {
    blobs
        .iter()
        .filter(|&&blob| {
            let path = blob_path(&world.volume(), blob);
            path.exists() || path.with_extension("gc").exists()
        })
        .count()
}

/// Runs purger cycles, each followed by a GC cycle, until the account row is gone; returns the cycles run.
pub async fn drain(
    world: &PurgeWorld,
    purger: &DeletionPurger,
    sweeper: &GcSweeper,
    account: Uuid,
) -> usize {
    let mut conn = world.db.conn_as(MIGRATOR).await;
    for cycle in 1..=MAX_CYCLES {
        purger
            .run_cycle(world.clock.now_utc())
            .await
            .expect("purger cycle");
        if state(&mut conn, account).await.is_none() {
            return cycle;
        }
        sweeper
            .run_cycle(world.clock.now_utc())
            .await
            .expect("gc cycle");
    }
    panic!("the account is not gone after {MAX_CYCLES} cycles");
}

/// The longest open transaction of the API role on the test database right now.
pub async fn longest_transaction(conn: &mut PgConnection) -> Duration {
    let seconds: f64 = sqlx::query_scalar(
        "SELECT coalesce(max(extract(epoch FROM clock_timestamp() - xact_start)), 0)::float8 \
         FROM pg_stat_activity WHERE datname = current_database() AND usename = 'sunnycloud' \
         AND xact_start IS NOT NULL AND pid <> pg_backend_pid()",
    )
    .fetch_one(conn)
    .await
    .expect("read pg_stat_activity");
    Duration::from_secs_f64(seconds.max(0.0))
}
