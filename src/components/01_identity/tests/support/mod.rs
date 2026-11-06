//! Component test harness (01 `tests.md`, Test Data Management) on a real PostgreSQL 18 (`DATABASE_URL`, a
//! superuser URL).
//!
//! A template database holds the two roles' schema setup with every embedded migration applied by
//! `sc_db::Migrator`; its name carries a hash of the migration files, so an edited or added migration builds a
//! new template. Each test clones it into its own database. The component under test connects as `sunnycloud`,
//! fixtures and probes as `sunnycloud_migrator`, time comes from a `ManualClock`, and the providers point at
//! the URL of the mock identity provider (`MOCK_IDP_URL`; the service itself is built by the test epic).
//! The setup lock and the password are the ones the `sc_db` and `sc_audit` tests use, so all suites can share
//! one server.

#![allow(dead_code)]

pub mod auth;
pub mod fixtures;
mod secrets;
pub mod statements;

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sc_clock::{Clock, ManualClock, Timestamp};
use sc_config::{ApiSettings, ConfigSources, Secret, load};
use sc_db::{Db, DbConfig, DbRole, Migrator};
use sqlx::postgres::PgConnectOptions;
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use sunnycloud_identity::IdentityConfig;
use tokio::sync::OnceCell;
use uuid::Uuid;

const TEMPLATE_PREFIX: &str = "sc_identity_tpl_";
const PASSWORD: &str = "sc_db_test_password";
const SETUP_LOCK: i64 = 0x5cdb_0001;
const STALE_AFTER_SECS: u64 = 3600;
const IDLE_POLL: Duration = Duration::from_millis(50);
const IDLE_POLL_LIMIT: u32 = 200;

pub const API: &str = "sunnycloud";
pub const MIGRATOR: &str = "sunnycloud_migrator";
pub const DEFAULT_MOCK_IDP_URL: &str = "http://mock-idp:9000";

/// 2026-11-05T12:00:00Z
pub const START: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);

/// Durations of the harness section: milliseconds and seconds, never the production values.
pub const SHORT_DURATIONS: [(&str, &str); 5] = [
    ("SUNNYCLOUD_SESSION_CACHE_TTL", "200ms"),
    ("SUNNYCLOUD_REFRESH_REUSE_GRACE", "300ms"),
    ("SUNNYCLOUD_OAUTH_STATE_TTL", "1s"),
    ("SUNNYCLOUD_LOGIN_CODE_TTL", "1s"),
    ("SUNNYCLOUD_DEVICE_CODE_TTL", "2s"),
];

pub fn mock_idp_url() -> String {
    std::env::var("MOCK_IDP_URL").unwrap_or_else(|_| DEFAULT_MOCK_IDP_URL.to_owned())
}

fn admin_options(database: &str) -> PgConnectOptions {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "DATABASE_URL is not set: the identity tests need the URL of a PostgreSQL 18 superuser, \
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

async fn advisory(conn: &mut PgConnection, function: &str) {
    sqlx::query(AssertSqlSafe(format!("SELECT {function}($1)")))
        .bind(SETUP_LOCK)
        .execute(conn)
        .await
        .unwrap_or_else(|error| panic!("{function}: {error}"));
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs()
}

/// Hash of the migration files, so a changed set of migrations never reuses an old template.
fn migrations_hash() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.expect("migration entry").path())
        .collect();
    names.sort();
    let mut hasher = DefaultHasher::new();
    for path in names {
        hasher.write(path.file_name().expect("file name").as_encoded_bytes());
        hasher.write(&std::fs::read(&path).expect("read migration"));
    }
    format!("{:016x}", hasher.finish())
}

fn template_name() -> String {
    format!("{TEMPLATE_PREFIX}{}", migrations_hash())
}

static CLUSTER: OnceCell<()> = OnceCell::const_new();

async fn setup_cluster() {
    let mut conn = admin_conn("postgres").await;
    advisory(&mut conn, "pg_advisory_lock").await;
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
    advisory(&mut conn, "pg_advisory_unlock").await;
}

/// Test databases are named `sc_identity_t_<unix seconds>_<uuid>`; one left by a panicked test is dropped by
/// the next run an hour later. Templates of earlier migration sets are dropped at once.
async fn drop_stale_databases(conn: &mut PgConnection) {
    let current = template_name();
    let names: Vec<String> = sqlx::query_scalar(
        r"SELECT datname::text FROM pg_database
          WHERE datname LIKE 'sc\_identity\_t\_%' OR datname LIKE 'sc\_identity\_tpl\_%'",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("list test databases");
    let now = unix_seconds();
    for name in names {
        let stale = if name.starts_with(TEMPLATE_PREFIX) {
            name != current
        } else {
            name.split('_')
                .nth(3)
                .and_then(|seconds| seconds.parse::<u64>().ok())
                .is_some_and(|created| now.saturating_sub(created) > STALE_AFTER_SECS)
        };
        if stale {
            exec(conn, format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")).await;
        }
    }
}

/// Builds the template once per migration set: roles' schema setup, then every embedded migration through
/// the real `Migrator` as the migrator role. A database can be a template source only with no sessions on it.
async fn ensure_template(conn: &mut PgConnection) {
    let template = template_name();
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT FROM pg_database WHERE datname = $1)")
            .bind(&template)
            .fetch_one(&mut *conn)
            .await
            .expect("template lookup");
    if exists {
        return;
    }
    let build = format!("{template}_build");
    exec(
        conn,
        format!("DROP DATABASE IF EXISTS {build} WITH (FORCE)"),
    )
    .await;
    exec(conn, format!("CREATE DATABASE {build}")).await;
    let mut setup = admin_conn(&build).await;
    exec(
        &mut setup,
        format!(
            "ALTER SCHEMA public OWNER TO {MIGRATOR}; REVOKE CREATE ON SCHEMA public FROM PUBLIC"
        ),
    )
    .await;
    setup.close().await.expect("close template connection");
    let migrator = Db::connect(&db_config(&build), DbRole::Migrator)
        .await
        .expect("connect the template build as the migrator");
    Migrator::run_pending(&migrator)
        .await
        .expect("apply the embedded migrations to the template");
    drop(migrator);
    wait_for_no_sessions(conn, &build).await;
    exec(conn, format!("ALTER DATABASE {build} RENAME TO {template}")).await;
}

async fn sessions_on(conn: &mut PgConnection, database: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname = $1")
        .bind(database)
        .fetch_one(conn)
        .await
        .expect("count sessions")
}

/// Polls until the pool's connections are gone; a leak fails with the count.
async fn wait_for_no_sessions(conn: &mut PgConnection, database: &str) {
    let mut count = sessions_on(conn, database).await;
    for _ in 0..IDLE_POLL_LIMIT {
        if count == 0 {
            return;
        }
        tokio::time::sleep(IDLE_POLL).await;
        count = sessions_on(conn, database).await;
    }
    panic!("{count} connections to {database} are still open");
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

/// Loads the API settings the way the server does, with short millisecond durations (`test` environment),
/// and `extra` environment pairs on top.
pub fn load_settings(extra: &[(&str, &str)]) -> Arc<ApiSettings> {
    let mut sources = ConfigSources {
        secrets_dir: secrets::shared_dir(),
        environment: sc_config::Environment::Test,
        ..ConfigSources::default()
    };
    let required = [
        ("SUNNYCLOUD_PUBLIC_URL", "https://sunnycloud.test"),
        ("SUNNYCLOUD_GITHUB_CLIENT_ID", "gh-client"),
    ];
    for (key, value) in required.iter().chain(extra) {
        sources.env.insert((*key).to_owned(), (*value).to_owned());
    }
    load::<ApiSettings>(&sources)
        .unwrap_or_else(|error| panic!("the harness settings do not load: {error}"))
}

/// A valid identity section from `settings`, its providers pointed at the mock identity provider.
pub fn identity_config(settings: &ApiSettings) -> IdentityConfig {
    let mut config = IdentityConfig::from_settings(settings);
    let url = mock_idp_url();
    for provider in &mut config.providers {
        provider.issuer = Some(url.clone());
    }
    config
}

/// One database cloned from the migrated template.
pub struct TestDb {
    pub name: String,
}

impl TestDb {
    pub async fn clone_template() -> TestDb {
        CLUSTER.get_or_init(setup_cluster).await;
        let name = format!(
            "sc_identity_t_{}_{}",
            unix_seconds(),
            Uuid::new_v4().simple()
        );
        let mut conn = admin_conn("postgres").await;
        advisory(&mut conn, "pg_advisory_lock").await;
        exec(
            &mut conn,
            format!("CREATE DATABASE {name} TEMPLATE {}", template_name()),
        )
        .await;
        advisory(&mut conn, "pg_advisory_unlock").await;
        TestDb { name }
    }

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
}

/// The component under test on role `sunnycloud`, the manual clock and the identity section.
pub struct Harness {
    pub db: TestDb,
    pub api: Db,
    pub clock: Arc<ManualClock>,
    pub config: IdentityConfig,
}

impl Harness {
    pub async fn new() -> Harness {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let config = identity_config(&load_settings(&SHORT_DURATIONS));
        Harness {
            db,
            api,
            clock: Arc::new(ManualClock::new(START)),
            config,
        }
    }

    pub fn now_micros(&self) -> i64 {
        self.clock.now_utc().as_unix_micros()
    }

    /// Fixtures and probes run as the schema owner.
    pub async fn fixture_conn(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Releases the component's connections and drops the database. Returns the number of connections that
    /// were still open when the pool was released, after waiting for the server to see them close.
    pub async fn finish(self) -> i64 {
        let Harness { db, api, .. } = self;
        drop(api);
        let mut remaining = db.open_connections().await;
        for _ in 0..IDLE_POLL_LIMIT {
            if remaining == 0 {
                break;
            }
            tokio::time::sleep(IDLE_POLL).await;
            remaining = db.open_connections().await;
        }
        db.drop_db().await;
        remaining
    }
}
