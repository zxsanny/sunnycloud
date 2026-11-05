//! Test cluster, template and per-test databases.
//!
//! The cluster gets the two roles of ADR-014 exactly as `deploy/postgres/initdb.d/10-roles.sh` creates them
//! (same attributes, schema `public` owned by the migrator, `CREATE` revoked from PUBLIC), plus three
//! login roles that must be refused as the API role. A template database holds that schema setup; every
//! test clones it, creates its tables as the migrator and drops the clone at the end.

use std::time::{SystemTime, UNIX_EPOCH};

use sc_config::Secret;
use sqlx::postgres::PgConnectOptions;
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::{Db, DbConfig, DbError, DbRole};

const TEMPLATE: &str = "sc_db_template_v1";
const TEMPLATE_BUILD: &str = "sc_db_template_v1_build";
const PASSWORD: &str = "sc_db_test_password";
const SETUP_LOCK: i64 = 0x5cdb_0001;
const STALE_AFTER_SECS: u64 = 3600;

pub const API: &str = "sunnycloud";
pub const MIGRATOR: &str = "sunnycloud_migrator";
pub const SUPER: &str = "sc_test_super";
pub const BYPASS: &str = "sc_test_bypass";
pub const OWNER: &str = "sc_test_owner";

const NOTES_SCHEMA: &str = r"
CREATE TABLE notes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id uuid NOT NULL,
    body text NOT NULL,
    CONSTRAINT notes_account_body_key UNIQUE (account_id, body),
    CONSTRAINT notes_body_short CHECK (length(body) <= 20)
);
CREATE TABLE note_refs (
    id serial PRIMARY KEY,
    note_id uuid NOT NULL REFERENCES notes (id)
);
ALTER TABLE notes ENABLE ROW LEVEL SECURITY;
CREATE POLICY notes_tenant ON notes USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
GRANT SELECT, INSERT, UPDATE, DELETE ON notes, note_refs TO sunnycloud;
GRANT USAGE ON SEQUENCE note_refs_id_seq TO sunnycloud;
";

pub fn admin_options(database: &str) -> PgConnectOptions {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "DATABASE_URL is not set: the sc_db tests need the URL of a PostgreSQL 18 superuser, \
             e.g. postgres://postgres:postgres@127.0.0.1:5432/postgres"
        )
    });
    url.parse::<PgConnectOptions>()
        .unwrap_or_else(|error| panic!("DATABASE_URL is not a postgres URL: {error}"))
        .database(database)
}

pub async fn admin_conn(database: &str) -> PgConnection {
    PgConnection::connect_with(&admin_options(database))
        .await
        .unwrap_or_else(|error| panic!("cannot connect to {database} as the superuser: {error}"))
}

pub fn role_url(database: &str, user: &str, host: &str, port: u16) -> String {
    format!("postgres://{user}:{PASSWORD}@{host}:{port}/{database}")
}

async fn exec(conn: &mut PgConnection, sql: String) {
    sqlx::raw_sql(AssertSqlSafe(sql.clone()))
        .execute(conn)
        .await
        .unwrap_or_else(|error| panic!("test setup statement failed: {error}\n{sql}"));
}

static CLUSTER: OnceCell<()> = OnceCell::const_new();

async fn ensure_cluster() {
    CLUSTER.get_or_init(setup_cluster).await;
}

async fn setup_cluster() {
    let mut conn = admin_conn("postgres").await;
    lock(&mut conn).await;
    create_roles(&mut conn).await;
    drop_stale_databases(&mut conn).await;
    ensure_template(&mut conn).await;
    unlock(&mut conn).await;
}

async fn lock(conn: &mut PgConnection) {
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SETUP_LOCK)
        .execute(conn)
        .await
        .expect("setup lock");
}

async fn unlock(conn: &mut PgConnection) {
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SETUP_LOCK)
        .execute(conn)
        .await
        .expect("setup unlock");
}

async fn create_roles(conn: &mut PgConnection) {
    let app = "LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION";
    let roles = [
        (MIGRATOR, app),
        (API, app),
        (SUPER, "LOGIN SUPERUSER"),
        (BYPASS, "LOGIN NOSUPERUSER BYPASSRLS"),
        (OWNER, app),
    ];
    for (role, attributes) in roles {
        exec(
            conn,
            format!(
                "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{role}') \
                 THEN CREATE ROLE {role} {attributes}; END IF; END $$; \
                 ALTER ROLE {role} {attributes} PASSWORD '{PASSWORD}'"
            ),
        )
        .await;
    }
}

/// Test databases are named `sc_db_t_<unix seconds>_<uuid>`; one left by a panicked test is dropped by the
/// next run an hour later.
async fn drop_stale_databases(conn: &mut PgConnection) {
    let names: Vec<String> = sqlx::query_scalar(
        r"SELECT datname::text FROM pg_database WHERE datname LIKE 'sc\_db\_t\_%'",
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
    /// Clone without tables.
    pub async fn empty() -> TestDb {
        ensure_cluster().await;
        let name = format!("sc_db_t_{}_{}", unix_seconds(), Uuid::new_v4().simple());
        let mut conn = admin_conn("postgres").await;
        lock(&mut conn).await;
        exec(
            &mut conn,
            format!("CREATE DATABASE {name} TEMPLATE {TEMPLATE}"),
        )
        .await;
        unlock(&mut conn).await;
        TestDb { name }
    }

    /// Clone with the `notes` table (row-level security on) and `note_refs`, created by the migrator.
    pub async fn with_notes() -> TestDb {
        let db = TestDb::empty().await;
        let options = admin_options(&db.name)
            .username(MIGRATOR)
            .password(PASSWORD);
        let mut conn = PgConnection::connect_with(&options)
            .await
            .expect("connect as the migrator");
        exec(&mut conn, NOTES_SCHEMA.to_string()).await;
        conn.close().await.expect("close migrator connection");
        db
    }

    pub async fn admin(&self) -> PgConnection {
        admin_conn(&self.name).await
    }

    /// URLs of the two real roles straight to the database. Pool 5, short timeouts that hold on a slow CI.
    pub fn config(&self) -> DbConfig {
        let options = admin_options(&self.name);
        self.config_via(options.get_host(), options.get_port())
    }

    /// The same, with every connection going through `host:port` (a proxy in front of the database).
    pub fn config_via(&self, host: &str, port: u16) -> DbConfig {
        let secret = |user| Secret::new(role_url(&self.name, user, host, port));
        DbConfig {
            db_api_url: secret(API),
            db_migrator_url: Some(secret(MIGRATOR)),
            db_pool_max: 5,
            db_acquire_timeout: std::time::Duration::from_secs(5),
            db_idle_in_tx_timeout: std::time::Duration::from_secs(15),
            db_statement_timeout: std::time::Duration::from_secs(30),
            db_recovery_budget: std::time::Duration::from_secs(60),
        }
    }

    pub async fn api(&self) -> Db {
        Db::connect(&self.config(), DbRole::Api)
            .await
            .expect("connect as the API role")
    }

    pub async fn count_notes(&self) -> i64 {
        let mut conn = self.admin().await;
        let count = sqlx::query_scalar("SELECT count(*) FROM notes")
            .fetch_one(&mut conn)
            .await
            .expect("count notes");
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

pub async fn insert_note(
    tx: &mut crate::DbTx,
    account: crate::AccountId,
    body: &str,
) -> Result<(), DbError> {
    sqlx::query("INSERT INTO notes (account_id, body) VALUES ($1, $2)")
        .bind(account)
        .bind(body)
        .execute(tx.conn())
        .await?;
    Ok(())
}
