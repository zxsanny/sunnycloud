//! A read-only look at the database for `doctor`: it reports a refused role or a stale schema instead of failing
//! to connect, which is what `Db::connect` does with them.

use sqlx::PgConnection;

use crate::config::{DbConfig, DbRole};
use crate::db::Db;
use crate::error::DbError;
use crate::migrate::{MigrationError, Migrator, SchemaState};
use crate::roles;

/// What one connection as the API role saw.
#[derive(Debug)]
pub struct DbInspection {
    /// `server_version` of the server.
    pub server_version: String,
    /// Why this connection may not serve as the API role (`Db::connect` would refuse it), if it may not.
    pub role_problem: Option<&'static str>,
    /// What `Migrator::check_version` answered.
    pub schema: Result<SchemaState, MigrationError>,
    /// The server's own clock, in microseconds since the Unix epoch.
    pub server_clock_micros: i64,
}

// `clock_timestamp()`, not `now()`: the point is the server's real clock, which the transaction-start time
// `now()` would not give; the application's own time still comes from `sc_clock`.
const SERVER_CLOCK: &str = "SELECT (extract(epoch FROM clock_timestamp()) * 1000000)::bigint";

impl Db {
    /// Connects as the API role without refusing a wrongly privileged one, reads the facts above and closes the
    /// pool. An unreachable database is `DbError::Unavailable`.
    pub async fn inspect(cfg: &DbConfig) -> Result<DbInspection, DbError> {
        let db = Db::open(cfg, DbRole::Api).await?;
        let seen = read(&db).await;
        db.close().await;
        seen
    }
}

async fn read(db: &Db) -> Result<DbInspection, DbError> {
    let mut conn = db.pool.acquire().await?;
    let facts = facts(&mut conn).await?;
    drop(conn);
    Ok(DbInspection {
        server_version: facts.0,
        role_problem: facts.1,
        schema: Migrator::check_version(db).await,
        server_clock_micros: facts.2,
    })
}

async fn facts(conn: &mut PgConnection) -> Result<(String, Option<&'static str>, i64), DbError> {
    let version = sqlx::query_scalar("SELECT current_setting('server_version')")
        .fetch_one(&mut *conn)
        .await?;
    let problem = roles::problem(conn, DbRole::Api).await?;
    let clock = sqlx::query_scalar(SERVER_CLOCK).fetch_one(conn).await?;
    Ok((version, problem, clock))
}
