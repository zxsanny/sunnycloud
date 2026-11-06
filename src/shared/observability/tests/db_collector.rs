//! The database collector against a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL, as the `sc_db` and
//! `sc_audit` tests): `db_up`, `db_pool_connections{state}`, `db_query_duration_seconds{op}`,
//! `migrations_pending`.

use std::time::Duration;

use prometheus_parse::{Scrape, Value};
use sc_config::Secret;
use sc_db::{Db, DbConfig, DbRole, Scope};
use sc_observability::{BuildInfo, Metrics, db_collector};
use sqlx::postgres::PgConnectOptions;
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use uuid::Uuid;

const PASSWORD: &str = "sc_db_test_password";

async fn api_db() -> Db {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        panic!(
            "DATABASE_URL is not set: the db collector test needs the URL of a PostgreSQL 18 superuser, \
             e.g. postgres://postgres:postgres@127.0.0.1:5432/postgres"
        )
    });
    let admin: PgConnectOptions = url.parse().expect("DATABASE_URL is a postgres URL");
    let mut conn = PgConnection::connect_with(&admin)
        .await
        .expect("superuser connection");
    sqlx::query("SELECT pg_advisory_lock(1552)")
        .execute(&mut conn)
        .await
        .expect("lock");
    let attributes = "LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION";
    sqlx::raw_sql(AssertSqlSafe(format!(
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'sunnycloud') \
         THEN CREATE ROLE sunnycloud {attributes}; END IF; END $$; \
         ALTER ROLE sunnycloud {attributes} PASSWORD '{PASSWORD}'"
    )))
    .execute(&mut conn)
    .await
    .expect("api role");
    sqlx::query("SELECT pg_advisory_unlock(1552)")
        .execute(&mut conn)
        .await
        .expect("unlock");
    let api_url = format!(
        "postgres://sunnycloud:{PASSWORD}@{}:{}/{}",
        admin.get_host(),
        admin.get_port(),
        admin.get_database().unwrap_or("postgres")
    );
    let cfg = DbConfig {
        db_api_url: Secret::new(api_url),
        db_migrator_url: None,
        db_pool_max: 4,
        db_acquire_timeout: Duration::from_secs(2),
        db_idle_in_tx_timeout: Duration::from_secs(5),
        db_statement_timeout: Duration::from_secs(5),
        db_recovery_budget: Duration::from_secs(5),
    };
    Db::connect(&cfg, DbRole::Api)
        .await
        .expect("api connection")
}

fn sample(scrape: &Scrape, name: &str, label: Option<(&str, &str)>) -> Option<Value> {
    scrape
        .samples
        .iter()
        .find(|sample| {
            sample.metric == name
                && label.is_none_or(|(key, value)| sample.labels.get(key) == Some(value))
        })
        .map(|sample| sample.value.clone())
}

fn number(value: Option<Value>) -> f64 {
    match value {
        Some(Value::Gauge(v) | Value::Counter(v) | Value::Untyped(v)) => v,
        other => panic!("not a plain number: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn db_families_come_from_the_real_pool_and_probe() {
    let db = api_db().await;
    db.run(Scope::Account(Uuid::max()), async |_tx| {
        Ok::<_, sc_db::DbError>(())
    })
    .await
    .expect("a unit of work");
    let metrics = Metrics::new(BuildInfo {
        version: "0",
        commit: "0",
        test_clock: false,
    })
    .expect("metrics");
    db_collector(db).register(&metrics).expect("register");

    let text = metrics.render().await.expect("render");
    let scrape = Scrape::parse(text.lines().map(|line| Ok(line.to_owned()))).expect("parses");

    assert_eq!(
        number(sample(&scrape, "sunnycloud_db_up", None)),
        1.0,
        "{text}"
    );
    assert_eq!(
        number(sample(
            &scrape,
            "sunnycloud_db_pool_connections",
            Some(("state", "max"))
        )),
        4.0
    );
    for state in ["in_use", "idle"] {
        assert!(
            number(sample(
                &scrape,
                "sunnycloud_db_pool_connections",
                Some(("state", state))
            )) >= 0.0
        );
    }
    assert!(
        text.contains("# TYPE sunnycloud_db_query_duration_seconds summary"),
        "{text}"
    );
    assert!(
        text.contains("sunnycloud_db_query_duration_seconds_count{op=\"run\"} 1"),
        "{text}"
    );
    assert!(text.contains("sunnycloud_db_query_duration_seconds_sum{op=\"run\"}"));
    assert!(
        number(sample(&scrape, "sunnycloud_migrations_pending", None)) >= 0.0,
        "the probe read the bookkeeping table"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn registering_the_db_collector_twice_is_a_duplicate() {
    let db = api_db().await;
    let metrics = Metrics::new(BuildInfo {
        version: "0",
        commit: "0",
        test_clock: false,
    })
    .expect("metrics");
    db_collector(db.clone()).register(&metrics).expect("first");

    let second = db_collector(db).register(&metrics);

    assert!(
        matches!(second, Err(sc_observability::ObsError::Duplicate(_))),
        "{second:?}"
    );
}
