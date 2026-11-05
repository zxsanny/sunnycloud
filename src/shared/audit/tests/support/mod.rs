//! Test cluster and per-test databases on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).
//!
//! The roles are created as `deploy/postgres/initdb.d/10-roles.sh` creates them (same attributes, schema
//! `public` owned by the migrator, `CREATE` revoked from PUBLIC). A template database holds that setup; each
//! test clones it and applies the embedded migrations through `sc_db::Migrator` as the migrator, so the
//! tables, owners and grants are exactly the shipped ones. The setup lock and the password are the ones the
//! `sc_db` tests use, so both suites can run against one server.

#![allow(dead_code)]

pub mod logs;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sc_audit::{AuditEvent, AuditEventKind, AuditWriter};
use sc_clock::{ManualClock, Timestamp};
use sc_config::Secret;
use sc_db::{Db, DbConfig, DbRole, Migrator};
use sqlx::postgres::PgConnectOptions;
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use tokio::sync::OnceCell;
use uuid::Uuid;

const TEMPLATE: &str = "sc_audit_template_v1";
const TEMPLATE_BUILD: &str = "sc_audit_template_v1_build";
const PASSWORD: &str = "sc_db_test_password";
const SETUP_LOCK: i64 = 0x5cdb_0001;
const STALE_AFTER_SECS: u64 = 3600;

pub const API: &str = "sunnycloud";
pub const MIGRATOR: &str = "sunnycloud_migrator";

/// 2026-11-05T12:00:00Z
pub const START: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);

fn admin_options(database: &str) -> PgConnectOptions {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "DATABASE_URL is not set: the sc_audit tests need the URL of a PostgreSQL 18 superuser, \
             e.g. postgres://postgres:postgres@127.0.0.1:5432/postgres"
        )
    });
    url.parse::<PgConnectOptions>()
        .unwrap_or_else(|error| panic!("DATABASE_URL is not a postgres URL: {error}"))
        .database(database)
}

async fn admin_conn(database: &str) -> PgConnection {
    PgConnection::connect_with(&admin_options(database))
        .await
        .unwrap_or_else(|error| panic!("cannot connect to {database} as the superuser: {error}"))
}

async fn exec(conn: &mut PgConnection, sql: String) {
    sqlx::raw_sql(AssertSqlSafe(sql.clone()))
        .execute(conn)
        .await
        .unwrap_or_else(|error| panic!("test setup statement failed: {error}\n{sql}"));
}

static CLUSTER: OnceCell<()> = OnceCell::const_new();

async fn setup_cluster() {
    let mut conn = admin_conn("postgres").await;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut conn)
        .await
        .expect("setup lock");
    let app = "LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION";
    for role in [MIGRATOR, API] {
        exec(
            &mut conn,
            format!(
                "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{role}') \
                 THEN CREATE ROLE {role} {app}; END IF; END $$; \
                 ALTER ROLE {role} {app} PASSWORD '{PASSWORD}'"
            ),
        )
        .await;
    }
    drop_stale_databases(&mut conn).await;
    ensure_template(&mut conn).await;
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(&mut conn)
        .await
        .expect("setup unlock");
}

/// Test databases are named `sc_audit_t_<unix seconds>_<uuid>`; one left by a panicked test is dropped by the
/// next run an hour later.
async fn drop_stale_databases(conn: &mut PgConnection) {
    let names: Vec<String> = sqlx::query_scalar(
        r"SELECT datname::text FROM pg_database WHERE datname LIKE 'sc\_audit\_t\_%'",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("list test databases");
    let now = unix_seconds();
    for name in names {
        let created = name
            .split('_')
            .nth(3)
            .and_then(|seconds| seconds.parse::<u64>().ok());
        if created.is_some_and(|created| now.saturating_sub(created) > STALE_AFTER_SECS) {
            exec(conn, format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
        }
    }
}

async fn ensure_template(conn: &mut PgConnection) {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT FROM pg_database WHERE datname = $1)")
            .bind(TEMPLATE)
            .fetch_one(&mut *conn)
            .await
            .expect("template lookup");
    if exists {
        return;
    }
    exec(
        conn,
        format!("DROP DATABASE IF EXISTS {TEMPLATE_BUILD} WITH (FORCE)"),
    )
    .await;
    exec(conn, format!("CREATE DATABASE {TEMPLATE_BUILD}")).await;
    let mut build = admin_conn(TEMPLATE_BUILD).await;
    exec(
        &mut build,
        format!(
            "ALTER SCHEMA public OWNER TO {MIGRATOR}; REVOKE CREATE ON SCHEMA public FROM PUBLIC"
        ),
    )
    .await;
    build.close().await.expect("close template connection");
    exec(
        conn,
        format!("ALTER DATABASE {TEMPLATE_BUILD} RENAME TO {TEMPLATE}"),
    )
    .await;
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs()
}

/// One database cloned from the template.
pub struct TestDb {
    pub name: String,
}

impl TestDb {
    /// A clone without tables.
    pub async fn empty() -> TestDb {
        CLUSTER.get_or_init(setup_cluster).await;
        let name = format!("sc_audit_t_{}_{}", unix_seconds(), Uuid::new_v4().simple());
        let mut conn = admin_conn("postgres").await;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(SETUP_LOCK)
            .execute(&mut conn)
            .await
            .expect("clone lock");
        exec(
            &mut conn,
            format!("CREATE DATABASE {name} TEMPLATE {TEMPLATE}"),
        )
        .await;
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(SETUP_LOCK)
            .execute(&mut conn)
            .await
            .expect("clone unlock");
        TestDb { name }
    }

    /// A clone with every embedded migration applied by the migrator role.
    pub async fn migrated() -> TestDb {
        let db = TestDb::empty().await;
        Migrator::run_pending(&db.migrator().await)
            .await
            .expect("apply the embedded migrations");
        db
    }

    pub async fn admin(&self) -> PgConnection {
        admin_conn(&self.name).await
    }

    /// Runs `sql` as the superuser (setup of a forced failure or a probe table).
    pub async fn admin_exec(&self, sql: &str) {
        let mut conn = self.admin().await;
        exec(&mut conn, sql.to_owned()).await;
        conn.close().await.expect("close admin connection");
    }

    pub async fn admin_count(&self, sql: &str) -> i64 {
        let mut conn = self.admin().await;
        let count = sqlx::query_scalar(AssertSqlSafe(sql.to_owned()))
            .fetch_one(&mut conn)
            .await
            .unwrap_or_else(|error| panic!("{error}\n{sql}"));
        conn.close().await.expect("close admin connection");
        count
    }

    pub fn config(&self) -> DbConfig {
        let options = admin_options(&self.name);
        let secret = |user| {
            Secret::new(format!(
                "postgres://{user}:{PASSWORD}@{}:{}/{}",
                options.get_host(),
                options.get_port(),
                self.name
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

    pub async fn drop_db(self) {
        let mut conn = admin_conn("postgres").await;
        exec(
            &mut conn,
            format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name),
        )
        .await;
    }
}

/// A writer on the API role with a clock that stands at [`START`] until ticked.
pub struct Fixture {
    pub db: TestDb,
    pub api: Db,
    pub clock: Arc<ManualClock>,
    pub writer: AuditWriter,
}

impl Fixture {
    pub async fn new() -> Fixture {
        let db = TestDb::migrated().await;
        let api = db.api().await;
        let clock = Arc::new(ManualClock::new(START));
        let writer = AuditWriter::new(api.clone(), clock.clone());
        Fixture {
            db,
            api,
            clock,
            writer,
        }
    }
}

pub fn event(kind: AuditEventKind) -> AuditEvent {
    AuditEvent::new(kind)
}
