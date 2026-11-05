//! Migration files that exist only in tests: the "binary with one more file" and the failure shapes.

use sqlx::SqlSafeStr;
use sqlx::migrate::{Migration, MigrationType};

pub fn migration(version: i64, kind: MigrationType, sql: &str) -> Migration {
    Migration::new(
        version,
        "seeded".into(),
        kind,
        sqlx::AssertSqlSafe(sql.to_string()).into_sql_str(),
        sql.starts_with("-- no-transaction"),
    )
}

pub fn pair(version: i64, up: &str, down: &str) -> Vec<Migration> {
    vec![
        migration(version, MigrationType::ReversibleUp, up),
        migration(version, MigrationType::ReversibleDown, down),
    ]
}

/// A table creation that a test can observe, with a plain `down`.
pub fn table_pair(version: i64, table: &str) -> Vec<Migration> {
    pair(
        version,
        &format!("CREATE TABLE {table} (id int PRIMARY KEY);"),
        &format!("DROP TABLE {table};"),
    )
}

pub const IRREVERSIBLE_DOWN: &str =
    "DO $$ BEGIN RAISE EXCEPTION 'irreversible: restore from backup'; END $$;";

/// While the superuser-owned table `sc_slow_gate` exists the `up` sleeps a minute (long enough to be killed);
/// without it the `up` finishes at once. The marker comment finds the backend in `pg_stat_activity`.
pub const SLOW_GATE: &str =
    "SELECT pg_sleep(CASE WHEN to_regclass('sc_slow_gate') IS NOT NULL THEN 60 ELSE 0 END);";
pub const SLOW_MARKER: &str = "-- sc_slow_migration_marker";
