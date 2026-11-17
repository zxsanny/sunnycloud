//! `Db::inspect`: the facts `doctor` reports, read even when `Db::connect` would refuse the role.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::support::{API, BYPASS, TestDb, admin_options, role_url};
use crate::{Db, DbConfig, DbError, Migrator, SchemaState};
use sc_config::Secret;

fn config_as(db: &TestDb, user: &str) -> DbConfig {
    let options = admin_options(&db.name);
    let url = role_url(&db.name, user, options.get_host(), options.get_port());
    DbConfig {
        db_api_url: Secret::new(url),
        ..db.config()
    }
}

fn unix_micros() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch");
    i64::try_from(since.as_micros()).expect("fits")
}

#[tokio::test]
async fn a_healthy_database_gives_version_18_no_role_problem_and_the_server_clock() {
    // Arrange
    let db = TestDb::empty().await;
    let migrator = db.migrator().await;
    Migrator::run_pending(&migrator).await.expect("migrate");

    // Act
    let seen = Db::inspect(&db.config()).await.expect("inspect");

    // Assert
    assert!(
        seen.server_version.starts_with("18"),
        "{}",
        seen.server_version
    );
    assert_eq!(seen.role_problem, None);
    assert!(
        matches!(seen.schema, Ok(SchemaState::Current)),
        "{:?}",
        seen.schema
    );
    let skew = (seen.server_clock_micros - unix_micros()).unsigned_abs();
    assert!(skew < Duration::from_secs(5).as_micros().try_into().expect("fits"));
    db.drop_db().await;
}

#[tokio::test]
async fn a_role_connect_refuses_is_reported_not_raised() {
    // Arrange
    let db = TestDb::empty().await;

    // Act
    let bypass = Db::inspect(&config_as(&db, BYPASS)).await.expect("inspect");

    // Assert
    assert!(bypass.role_problem.is_some());
    db.drop_db().await;
}

#[tokio::test]
async fn an_api_role_that_owns_the_schema_is_named() {
    // Arrange
    let db = TestDb::empty().await;
    let mut admin = db.admin().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "ALTER SCHEMA public OWNER TO {API}"
    )))
    .execute(&mut admin)
    .await
    .expect("hand the schema to the API role");

    // Act
    let seen = Db::inspect(&db.config()).await.expect("inspect");

    // Assert
    assert_eq!(
        seen.role_problem,
        Some("the API role owns the application schema")
    );
    db.drop_db().await;
}

#[tokio::test]
async fn pending_migrations_are_reported_in_the_schema_field() {
    // Arrange
    let db = TestDb::empty().await;

    // Act
    let pending = Db::inspect(&db.config()).await.expect("inspect");

    // Assert
    assert!(
        matches!(pending.schema, Ok(SchemaState::Pending { .. })),
        "{:?}",
        pending.schema
    );
    db.drop_db().await;
}

#[tokio::test]
async fn an_unreachable_database_is_unavailable() {
    // Arrange
    let db = TestDb::empty().await;
    let mut cfg = db.config_via("127.0.0.1", 1);
    cfg.db_acquire_timeout = Duration::from_secs(1);

    // Act
    let result = Db::inspect(&cfg).await;

    // Assert
    assert_eq!(result.err(), Some(DbError::Unavailable));
    db.drop_db().await;
}
