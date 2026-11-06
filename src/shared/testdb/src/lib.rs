//! Per-test PostgreSQL databases on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL). Dev-dependency only.
//!
//! The two roles are created as `deploy/postgres/initdb.d/10-roles.sh` creates them (same attributes, schema
//! `public` owned by the migrator, `CREATE` revoked from PUBLIC). A template database holds that setup with every
//! embedded migration applied by `sc_db::Migrator` as the migrator, so the tables, owners and grants are exactly
//! the shipped ones; its name carries a hash of the migration files, so an edited or added migration builds a new
//! template. [`TestDb::clone_template`] gives each test its own clone.
//!
//! `sc_db`'s own lib tests cannot use this crate (it depends on `sc_db`); `src/shared/db/src/tests/support.rs`
//! mirrors the constants kept here: the role password `sc_db_test_password` and the setup advisory lock
//! `0x5cdb_0001`. Keep them equal, so every suite can run in parallel against one server.

mod cluster;

use std::sync::Arc;
use std::time::Duration;

use cluster::{
    IDLE_POLL, IDLE_POLL_LIMIT, NAME_PREFIX, PASSWORD, admin_conn, admin_options, ensure_cluster,
    exec, exec_locked, prepare_schema, sessions_on, template_name, unix_seconds,
};
use sc_clock::{ManualClock, Timestamp};
use sc_config::Secret;
use sc_db::{Db, DbConfig, DbRole};
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use uuid::Uuid;

pub const API: &str = "sunnycloud";
pub const MIGRATOR: &str = "sunnycloud_migrator";

/// 2026-11-05T12:00:00Z
pub const START: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);

/// A clock that stands at [`START`] until ticked.
pub fn clock_at_start() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(START))
}

fn db_config(database: &str) -> DbConfig {
    let options = admin_options(database);
    let secret = |user| {
        Secret::new(format!(
            "postgres://{user}:{PASSWORD}@{}:{}/{database}",
            options.get_host(),
            options.get_port(),
        ))
    };
    DbConfig {
        db_api_url: secret(API),
        db_migrator_url: Some(secret(MIGRATOR)),
        db_pool_max: 5,
        db_acquire_timeout: Duration::from_secs(5),
        db_idle_in_tx_timeout: Duration::from_secs(15),
        db_statement_timeout: Duration::from_secs(30),
        db_recovery_budget: Duration::from_secs(60),
    }
}

/// One database of its own; drop it with [`TestDb::drop_db`] or [`TestDb::finish`].
pub struct TestDb {
    pub name: String,
}

impl TestDb {
    /// A clone of the migrated template.
    pub async fn clone_template() -> TestDb {
        let db = TestDb::named().await;
        exec_locked(format!(
            "CREATE DATABASE {} TEMPLATE {}",
            db.name,
            template_name()
        ))
        .await;
        db
    }

    /// A database with the roles' schema setup and no tables, for tests that apply migrations themselves.
    pub async fn clone_empty() -> TestDb {
        let db = TestDb::named().await;
        exec_locked(format!("CREATE DATABASE {}", db.name)).await;
        prepare_schema(&db.name).await;
        db
    }

    async fn named() -> TestDb {
        ensure_cluster().await;
        TestDb {
            name: format!(
                "{NAME_PREFIX}{}_{}",
                unix_seconds(),
                Uuid::new_v4().simple()
            ),
        }
    }

    /// URLs of the two real roles straight to the database. Pool 5, short timeouts that hold on a slow CI.
    pub fn config(&self) -> DbConfig {
        db_config(&self.name)
    }

    pub async fn api(&self) -> Db {
        Db::connect(&self.config(), DbRole::Api)
            .await
            .expect("connect as the API role")
    }

    pub async fn migrator(&self) -> Db {
        Db::connect(&self.config(), DbRole::Migrator)
            .await
            .expect("connect as the migrator role")
    }

    /// A plain connection as `user`; no scope is set on it.
    pub async fn conn_as(&self, user: &str) -> PgConnection {
        let options = admin_options(&self.name).username(user).password(PASSWORD);
        PgConnection::connect_with(&options)
            .await
            .unwrap_or_else(|error| panic!("cannot connect to {} as {user}: {error}", self.name))
    }

    /// A superuser connection to this database.
    pub async fn admin(&self) -> PgConnection {
        admin_conn(&self.name).await
    }

    /// Runs `sql` as the superuser (setup of a forced failure or a probe table).
    pub async fn admin_exec(&self, sql: &str) {
        let mut conn = self.admin().await;
        exec(&mut conn, sql.to_owned()).await;
        conn.close().await.expect("close admin connection");
    }

    /// The single `bigint` a superuser query returns.
    pub async fn admin_count(&self, sql: &str) -> i64 {
        let mut conn = self.admin().await;
        let count = sqlx::query_scalar(AssertSqlSafe(sql.to_owned()))
            .fetch_one(&mut conn)
            .await
            .unwrap_or_else(|error| panic!("{error}\n{sql}"));
        conn.close().await.expect("close admin connection");
        count
    }

    /// Server-side sessions on this database right now.
    pub async fn open_connections(&self) -> i64 {
        let mut conn = admin_conn("postgres").await;
        let count = sessions_on(&mut conn, &self.name).await;
        conn.close().await.expect("close admin connection");
        count
    }

    pub async fn drop_db(self) {
        let mut conn = admin_conn("postgres").await;
        exec(
            &mut conn,
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        )
        .await;
    }

    /// Drops the database and returns the number of connections still open on it after waiting for the server to
    /// see the pools close; the caller drops its pools first.
    pub async fn finish(self) -> i64 {
        let mut remaining = self.open_connections().await;
        for _ in 0..IDLE_POLL_LIMIT {
            if remaining == 0 {
                break;
            }
            tokio::time::sleep(IDLE_POLL).await;
            remaining = self.open_connections().await;
        }
        self.drop_db().await;
        remaining
    }
}
