//! Cluster-level setup shared by every test database: the two roles, the migrated template and the stale-database sweep.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sc_db::{Db, DbRole, Migrator};
use sqlx::postgres::PgConnectOptions;
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use tokio::sync::OnceCell;

use crate::{API, MIGRATOR, db_config};

const TEMPLATE_PREFIX: &str = "sc_testdb_tpl_";
pub(crate) const NAME_PREFIX: &str = "sc_testdb_t_";
pub(crate) const PASSWORD: &str = "sc_db_test_password";
const SETUP_LOCK: i64 = 0x5cdb_0001;
const STALE_AFTER_SECS: u64 = 3600;
pub(crate) const IDLE_POLL: Duration = Duration::from_millis(50);
pub(crate) const IDLE_POLL_LIMIT: u32 = 200;

static CLUSTER: OnceCell<()> = OnceCell::const_new();

pub(crate) fn admin_options(database: &str) -> PgConnectOptions {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "DATABASE_URL is not set: the database tests need the URL of a PostgreSQL 18 superuser, \
             e.g. postgres://postgres:postgres@127.0.0.1:5432/postgres"
        )
    });
    url.parse::<PgConnectOptions>()
        .unwrap_or_else(|error| panic!("DATABASE_URL is not a postgres URL: {error}"))
        .database(database)
}

pub(crate) async fn admin_conn(database: &str) -> PgConnection {
    PgConnection::connect_with(&admin_options(database))
        .await
        .unwrap_or_else(|error| panic!("cannot connect to {database} as the superuser: {error}"))
}

pub(crate) async fn exec(conn: &mut PgConnection, sql: String) {
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

pub(crate) fn unix_seconds() -> u64 {
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

pub(crate) fn template_name() -> String {
    format!("{TEMPLATE_PREFIX}{}", migrations_hash())
}

/// Runs `sql` on a connection holding the setup lock, so concurrent suites never clone or create from the same
/// source database at once.
pub(crate) async fn exec_locked(sql: String) {
    ensure_cluster().await;
    let mut conn = admin_conn("postgres").await;
    advisory(&mut conn, "pg_advisory_lock").await;
    exec(&mut conn, sql).await;
    advisory(&mut conn, "pg_advisory_unlock").await;
}

pub(crate) async fn ensure_cluster() {
    CLUSTER.get_or_init(setup_cluster).await;
}

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

/// Test databases are named `sc_testdb_t_<unix seconds>_<uuid>`; one left by a panicked test is dropped by the
/// next run an hour later. Templates of earlier migration sets are dropped at once.
async fn drop_stale_databases(conn: &mut PgConnection) {
    let current = template_name();
    let names: Vec<String> = sqlx::query_scalar(
        r"SELECT datname::text FROM pg_database
          WHERE datname LIKE 'sc\_testdb\_t\_%' OR datname LIKE 'sc\_testdb\_tpl\_%'",
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

/// The schema setup of `deploy/postgres/initdb.d/10-roles.sh`: `public` owned by the migrator, `CREATE` revoked
/// from PUBLIC.
pub(crate) async fn prepare_schema(database: &str) {
    let mut conn = admin_conn(database).await;
    exec(
        &mut conn,
        format!(
            "ALTER SCHEMA public OWNER TO {MIGRATOR}; REVOKE CREATE ON SCHEMA public FROM PUBLIC"
        ),
    )
    .await;
    conn.close().await.expect("close schema setup connection");
}

/// Builds the template once per migration set: the schema setup, then every embedded migration through the real
/// `Migrator` as the migrator role. A database can be a template source only with no sessions on it.
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
    prepare_schema(&build).await;
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

pub(crate) async fn sessions_on(conn: &mut PgConnection, database: &str) -> i64 {
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
