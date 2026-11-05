//! AC-2 (cross-account reads return nothing) and AC-3 (scope never leaks).

use std::collections::HashSet;

use sqlx::Row;
use uuid::Uuid;

use super::support::{TestDb, insert_note};
use crate::{DbError, RunError, Scope};

async fn names(tx: &mut crate::DbTx) -> Result<Vec<(Uuid, String)>, DbError> {
    let rows = sqlx::query("SELECT account_id, body FROM notes ORDER BY body")
        .fetch_all(tx.conn())
        .await?;
    Ok(rows
        .iter()
        .map(|row| (row.get("account_id"), row.get("body")))
        .collect())
}

#[tokio::test]
async fn scope_a_reads_only_a_and_an_unset_scope_reads_nothing() {
    let db = TestDb::with_notes().await;
    let mut cfg = db.config();
    cfg.db_pool_max = 1;
    let api = crate::Db::connect(&cfg, crate::DbRole::Api)
        .await
        .expect("connect");
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    api.run_system("test_seed", async |tx| {
        for body in ["a1", "a2", "a3"] {
            insert_note(tx, a, body).await?;
        }
        for body in ["b1", "b2"] {
            insert_note(tx, b, body).await?;
        }
        Ok::<_, DbError>(())
    })
    .await
    .expect("seed in system scope");

    let in_a = api.run(Scope::Account(a), names).await.expect("scope A");
    let in_b = api.run(Scope::Account(b), names).await.expect("scope B");

    assert_eq!(in_a.len(), 3);
    assert!(in_a.iter().all(|(account, _)| *account == a));
    assert_eq!(in_b.len(), 2);
    assert!(in_b.iter().all(|(account, _)| *account == b));
    let system = api.run_system("test_read", names).await.expect("system");
    assert_eq!(system.len(), 5);
    db.drop_db().await;
}

#[tokio::test]
async fn statement_without_scope_sees_no_rows_on_fresh_and_used_connections() {
    let db = TestDb::with_notes().await;
    let api = db.api().await;
    let a = Uuid::new_v4();
    api.run(Scope::Account(a), async |tx| insert_note(tx, a, "a1").await)
        .await
        .expect("seed");
    let fresh_pool_count: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
        .fetch_one(&api.pool)
        .await
        .expect("unscoped statement");
    // The connection that served the scoped runs reads the setting back as '' (not NULL) once the
    // transaction ended; the policy must still return nothing and must not fail.
    let mut cfg = db.config();
    cfg.db_pool_max = 1;
    let single = crate::Db::connect(&cfg, crate::DbRole::Api)
        .await
        .expect("connect");
    single
        .run(Scope::Account(a), async |tx| names(tx).await.map(|_| ()))
        .await
        .expect("use the only connection");

    let used_pool_count: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
        .fetch_one(&single.pool)
        .await
        .expect("unscoped statement on a used connection");

    assert_eq!(fresh_pool_count, 0);
    assert_eq!(used_pool_count, 0);
    db.drop_db().await;
}

#[tokio::test]
async fn scope_a_cannot_write_or_see_account_b_rows() {
    let db = TestDb::with_notes().await;
    let api = db.api().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    api.run(Scope::Account(b), async |tx| {
        insert_note(tx, b, "secret").await
    })
    .await
    .expect("seed B");

    let write_as_a = api
        .run(Scope::Account(a), async |tx| {
            insert_note(tx, b, "forged").await
        })
        .await;
    let delete_as_a = api
        .run(Scope::Account(a), async |tx| {
            let done = sqlx::query("DELETE FROM notes WHERE account_id = $1")
                .bind(b)
                .execute(tx.conn())
                .await?;
            Ok::<_, DbError>(done.rows_affected())
        })
        .await;

    assert!(
        matches!(write_as_a, Err(RunError::Closure(DbError::Other(_)))),
        "{write_as_a:?}"
    );
    assert_eq!(delete_as_a.expect("delete runs"), 0);
    assert_eq!(db.count_notes().await, 1);
    db.drop_db().await;
}

async fn settings(conn: &mut sqlx::PgConnection) -> (i32, String, String, String) {
    let row = sqlx::query(
        "SELECT pg_backend_pid(), \
                coalesce(current_setting('sunnycloud.account_id', true), ''), \
                coalesce(current_setting('sunnycloud.scope', true), ''), \
                current_setting('application_name')",
    )
    .fetch_one(conn)
    .await
    .expect("read settings");
    (row.get(0), row.get(1), row.get(2), row.get(3))
}

#[tokio::test]
async fn scope_is_empty_on_every_connection_after_a_thousand_runs() {
    let db = TestDb::empty().await;
    let api = db.api().await;
    let accounts = [Uuid::new_v4(), Uuid::new_v4()];
    let before = {
        let mut conn = api.pool.acquire().await.expect("acquire");
        settings(&mut conn).await
    };

    let mut tasks = Vec::new();
    for worker in 0..20 {
        let api = api.clone();
        tasks.push(tokio::spawn(async move {
            for round in 0..50 {
                let account = accounts[(worker + round) % 2];
                if round % 10 == 9 {
                    let seen = api
                        .run_system("test_scope", async |tx| {
                            Ok::<_, DbError>(settings(tx.conn()).await)
                        })
                        .await
                        .expect("system run");
                    assert_eq!((seen.2.as_str(), seen.3.as_str()), ("system", "test_scope"));
                } else {
                    let seen = api
                        .run(Scope::Account(account), async |tx| {
                            Ok::<_, DbError>(settings(tx.conn()).await)
                        })
                        .await
                        .expect("scoped run");
                    assert_eq!(seen.1, account.to_string());
                }
            }
        }));
    }
    for task in tasks {
        task.await.expect("worker task");
    }

    let mut held = Vec::new();
    for _ in 0..5 {
        held.push(api.pool.acquire().await.expect("acquire every connection"));
    }
    let mut backends = HashSet::new();
    for conn in &mut held {
        let (pid, account, scope, application) = settings(conn).await;
        assert_eq!(account, "", "tenant setting leaked on backend {pid}");
        assert_eq!(scope, "", "system setting leaked on backend {pid}");
        assert_eq!(
            application, before.3,
            "operation label leaked on backend {pid}"
        );
        backends.insert(pid);
    }
    assert_eq!(
        backends.len(),
        5,
        "every pooled connection must have been inspected"
    );
    drop(held);
    db.drop_db().await;
}
