//! AC-6 (idle transactions are killed), AC-7 (error mapping) and AC-8 (recovery).

use std::time::{Duration, Instant};

use uuid::Uuid;

use super::proxy::Proxy;
use super::support::{TestDb, admin_options};
use crate::{Db, DbConfig, DbError, DbRole, RunError, Scope};

const ERRORS_SQL: [&str; 4] = [
    "INSERT INTO notes (account_id, body) VALUES ($1, 'dup'), ($1, 'dup')",
    "INSERT INTO note_refs (note_id) VALUES (gen_random_uuid())",
    "INSERT INTO notes (account_id, body) VALUES ($1, 'this body is far too long for the check')",
    "DO $$ BEGIN RAISE EXCEPTION 'retry me' USING ERRCODE = '40001'; END $$",
];

#[tokio::test]
async fn server_errors_map_to_their_variants() {
    let db = TestDb::with_notes().await;
    let api = db.api().await;
    let account = Uuid::new_v4();
    let mut mapped = Vec::new();

    for sql in ERRORS_SQL {
        let result = api
            .run(Scope::Account(account), async |tx| {
                let statement = sqlx::query(sql);
                let statement = if sql.contains("$1") {
                    statement.bind(account)
                } else {
                    statement
                };
                statement.execute(tx.conn()).await?;
                Ok::<_, DbError>(())
            })
            .await;
        mapped.push(result.expect_err("the statement raises"));
    }

    let closure_errors: Vec<DbError> = mapped
        .into_iter()
        .map(|error| match error {
            RunError::Closure(error) => error,
            RunError::Db(error) => panic!("expected the closure's error, got Db({error})"),
        })
        .collect();
    assert_eq!(
        closure_errors,
        [
            DbError::UniqueViolation {
                constraint: Some("notes_account_body_key".into())
            },
            DbError::ForeignKeyViolation,
            DbError::CheckViolation,
            DbError::Serialization,
        ]
    );
    assert_eq!(db.count_notes().await, 0);
    db.drop_db().await;
}

#[tokio::test]
async fn killed_backend_is_unavailable() {
    let db = TestDb::empty().await;
    let api = db.api().await;
    let mut admin = db.admin().await;

    let result = api
        .run_system("test_kill", async |tx| {
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(tx.conn())
                .await?;
            sqlx::query("SELECT pg_terminate_backend($1)")
                .bind(pid)
                .execute(&mut admin)
                .await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
            sqlx::query("SELECT 1").execute(tx.conn()).await?;
            Ok::<_, DbError>(())
        })
        .await;

    assert!(
        matches!(
            result,
            Err(RunError::Closure(DbError::Unavailable)) | Err(RunError::Db(DbError::Unavailable))
        ),
        "{result:?}"
    );
    api.run_system("test_after_kill", async |tx| {
        sqlx::query("SELECT 1").execute(tx.conn()).await?;
        Ok::<_, DbError>(())
    })
    .await
    .expect("the pool replaces the killed connection");
    db.drop_db().await;
}

#[tokio::test]
async fn idle_transaction_is_terminated_by_the_server() {
    let db = TestDb::empty().await;
    let cfg = DbConfig {
        db_idle_in_tx_timeout: Duration::from_millis(500),
        ..db.config()
    };
    let api = Db::connect(&cfg, DbRole::Api).await.expect("connect");
    let mut next_statement = None;

    let result = api
        .run_system("test_idle", async |tx| {
            sqlx::query("SELECT 1").execute(tx.conn()).await?;
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let after = sqlx::query("SELECT 1")
                .execute(tx.conn())
                .await
                .map(|_| ())
                .map_err(DbError::from);
            next_statement = Some(after.clone());
            after
        })
        .await;

    assert_eq!(next_statement, Some(Err(DbError::Unavailable)));
    assert!(result.is_err());
    let mut admin = db.admin().await;
    let idle: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity WHERE datname = $1 AND state = 'idle in transaction'",
    )
    .bind(&db.name)
    .fetch_one(&mut admin)
    .await
    .expect("count idle transactions");
    assert_eq!(idle, 0);
    db.drop_db().await;
}

#[tokio::test]
async fn pool_recovers_within_the_budget_after_the_database_returns() {
    let db = TestDb::empty().await;
    let target = admin_options(&db.name);
    let proxy = Proxy::start(target.get_host(), target.get_port()).await;
    let cfg = DbConfig {
        db_acquire_timeout: Duration::from_secs(1),
        ..db.config_via("127.0.0.1", proxy.port())
    };
    let api = Db::connect(&cfg, DbRole::Api).await.expect("connect");
    let ping = async |api: &Db| {
        api.run(Scope::Account(Uuid::new_v4()), async |tx| {
            sqlx::query("SELECT 1").execute(tx.conn()).await?;
            Ok::<_, DbError>(())
        })
        .await
    };
    ping(&api).await.expect("healthy before the outage");

    proxy.cut();
    let during = ping(&api).await;
    proxy.restore().await;

    assert!(
        matches!(during, Err(RunError::Db(DbError::Unavailable))),
        "{during:?}"
    );
    let restored = Instant::now();
    loop {
        match ping(&api).await {
            Ok(()) => break,
            Err(error) => {
                assert!(
                    restored.elapsed() < cfg.db_recovery_budget,
                    "no successful run within the recovery budget, last error {error:?}"
                );
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
    drop(proxy);
    db.drop_db().await;
}
