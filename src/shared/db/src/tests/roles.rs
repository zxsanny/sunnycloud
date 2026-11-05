//! AC-1: wrong role refused.

use super::support::{self, API, BYPASS, MIGRATOR, OWNER, SUPER, TestDb};
use crate::{Db, DbConfig, DbError, DbRole, roles};
use sc_config::Secret;
use std::time::Duration;

fn config_as(db: &TestDb, user: &str) -> DbConfig {
    let options = support::admin_options(&db.name);
    let url = support::role_url(&db.name, user, options.get_host(), options.get_port());
    DbConfig {
        db_api_url: Secret::new(url.clone()),
        db_migrator_url: Some(Secret::new(url)),
        ..db.config()
    }
}

#[tokio::test]
async fn api_connect_refuses_superuser_bypassrls_and_schema_owner_roles() {
    let db = TestDb::empty().await;
    let mut admin = db.admin().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "ALTER SCHEMA public OWNER TO {OWNER}"
    )))
    .execute(&mut admin)
    .await
    .expect("hand the schema to a plain role");

    for user in [SUPER, BYPASS, OWNER] {
        let result = Db::connect(&config_as(&db, user), DbRole::Api).await;

        assert!(
            matches!(result, Err(DbError::WrongRole(_))),
            "{user} must be refused as the API role, got {result:?}"
        );
    }
    db.drop_db().await;
}

#[tokio::test]
async fn api_connect_refuses_the_real_role_when_it_owns_the_schema() {
    let db = TestDb::empty().await;
    let mut admin = db.admin().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "ALTER SCHEMA public OWNER TO {API}"
    )))
    .execute(&mut admin)
    .await
    .expect("hand the schema to the API role");

    let result = Db::connect(&config_as(&db, API), DbRole::Api).await;

    assert_eq!(
        result.err(),
        Some(DbError::WrongRole(
            "the API role owns the application schema"
        ))
    );
    db.drop_db().await;
}

#[tokio::test]
async fn api_connect_accepts_the_real_sunnycloud_role() {
    let db = TestDb::empty().await;

    let result = Db::connect(&config_as(&db, API), DbRole::Api).await;

    assert!(result.is_ok(), "{result:?}");
    db.drop_db().await;
}

#[tokio::test]
async fn migrator_connects_only_as_the_migrator_role() {
    let db = TestDb::empty().await;

    let as_migrator = Db::connect(&config_as(&db, MIGRATOR), DbRole::Migrator).await;
    let as_api = Db::connect(&config_as(&db, API), DbRole::Migrator).await;
    let as_other = Db::connect(&config_as(&db, OWNER), DbRole::Migrator).await;

    assert!(as_migrator.is_ok(), "{as_migrator:?}");
    assert!(matches!(as_api, Err(DbError::WrongRole(_))), "{as_api:?}");
    assert!(
        matches!(as_other, Err(DbError::WrongRole(_))),
        "{as_other:?}"
    );
    db.drop_db().await;
}

#[tokio::test]
async fn api_role_url_of_the_migrator_is_refused() {
    let db = TestDb::empty().await;

    let result = Db::connect(&config_as(&db, MIGRATOR), DbRole::Api).await;

    assert!(matches!(result, Err(DbError::WrongRole(_))), "{result:?}");
    db.drop_db().await;
}

#[tokio::test]
async fn migrator_role_without_a_migrator_url_is_a_configuration_failure() {
    let db = TestDb::empty().await;
    let cfg = DbConfig {
        db_migrator_url: None,
        ..db.config()
    };

    let result = Db::connect(&cfg, DbRole::Migrator).await;

    assert!(matches!(result, Err(DbError::Other(_))), "{result:?}");
    db.drop_db().await;
}

#[tokio::test]
async fn unreachable_database_is_unavailable() {
    let db = TestDb::empty().await;
    let mut cfg = db.config_via("127.0.0.1", 1);
    cfg.db_acquire_timeout = Duration::from_secs(1);

    let result = Db::connect(&cfg, DbRole::Api).await;

    assert_eq!(result.err(), Some(DbError::Unavailable));
    db.drop_db().await;
}

/// The name check refuses the three roles above before the privilege check can run, so each privilege check
/// is exercised on its own: `SET ROLE` makes the check see that role's attributes.
#[tokio::test]
async fn privilege_check_names_each_failed_attribute() {
    let db = TestDb::empty().await;
    let mut admin = db.admin().await;
    let cases = [
        (SUPER, Some("the API role is a superuser")),
        (BYPASS, Some("the API role has BYPASSRLS")),
        (API, None),
    ];

    for (role, expected) in cases {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET ROLE {role}")))
            .execute(&mut admin)
            .await
            .expect("set role");

        let problem = roles::api_privilege_problem(&mut admin).await;

        assert_eq!(problem, Ok(expected), "role {role}");
        sqlx::raw_sql("RESET ROLE")
            .execute(&mut admin)
            .await
            .expect("reset role");
    }
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "ALTER SCHEMA public OWNER TO {API}"
    )))
    .execute(&mut admin)
    .await
    .expect("hand the schema to the API role");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET ROLE {API}")))
        .execute(&mut admin)
        .await
        .expect("set role");

    let owner = roles::api_privilege_problem(&mut admin).await;

    assert_eq!(owner, Ok(Some("the API role owns the application schema")));
    db.drop_db().await;
}
