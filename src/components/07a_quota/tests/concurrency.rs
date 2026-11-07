//! AC-74, AC-1 and AC-2 of the quota concurrency suite: the seeded 1,000-transaction workload (20 waves of 50 on
//! one account, one pool connection per transaction) with a 5 ms sampler, an interleaved second account, and a
//! share of commit-shaped transactions rolled back to a savepoint. Waves start on a barrier and the lock-wait
//! bound is read from `pg_stat_activity`, so no assertion depends on wall time.

mod support;
#[path = "support/workload.rs"]
mod workload;

use contracts::QuotaLedger;
use sc_db::{DbError, Scope};
use sqlx::Connection;
use support::{Harness, MIB, account_with_quota, admin, ledger, quota_row};
use tokio::sync::oneshot;
use uuid::Uuid;
use workload::{LIMIT, RowSampler, WaitProbe, lock_waits_ms};

const POOL: u32 = 8;
const B_LABEL: &str = "sc_quota_b";
const B_ROUNDS: i64 = 100;
const LOCK_WAIT_BOUND_MS: f64 = 100.0;
const ROLLBACK_PERCENT: usize = 25;

const LABEL: &str = "SELECT set_config('application_name', $1, true)";

async fn label_session(tx: &mut sc_db::DbTx, label: &str) {
    sqlx::query(LABEL)
        .bind(label)
        .execute(tx.conn())
        .await
        .expect("label the session");
}

fn assert_never_above_limit(samples: &[(i64, i64)]) {
    assert!(samples.len() > 1, "{} samples", samples.len());
    assert!(
        samples
            .iter()
            .all(|(used, reserved)| *used <= LIMIT && used + reserved <= LIMIT),
        "{samples:?}"
    );
}

fn assert_clean(report: &workload::Report) {
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.granted > 0 && report.deleted > 0, "{report:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn thousand_commits_and_deletes_keep_usage_exact_and_under_the_limit() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, Some(LIMIT), 0, 0).await;
    let db = harness.api_with_pool(POOL).await;
    let sampler = RowSampler::start(harness.fixture_conn().await, account);

    // Act
    let report = workload::run(&db, account, 0).await;
    let samples = sampler.stop().await;
    let recomputes_during = db
        .stats()
        .queries
        .iter()
        .filter(|query| query.op == "quota_recompute")
        .count();
    let drift = admin(&db, LIMIT)
        .recompute_columns(account, report.ledger_sum, 0, false)
        .await
        .expect("recompute");

    // Assert
    assert_clean(&report);
    assert!(
        report.refused > 0,
        "the workload never reached the limit: {report:?}"
    );
    assert_eq!(
        quota_row(&mut conn, account).await,
        (Some(LIMIT), report.ledger_sum, 0)
    );
    assert_never_above_limit(&samples);
    assert_eq!(recomputes_during, 0);
    assert!(!drift.found(), "{drift:?}");
    assert_eq!(drift.used.stored, report.ledger_sum);
    assert_eq!(drift.reserved.stored, 0);
    conn.close().await.expect("close fixture connection");
    drop(db);
    harness.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_account_stays_exact_and_unblocked_under_the_workload() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    account_with_quota(&mut conn, a, Some(LIMIT), 0, 0).await;
    account_with_quota(&mut conn, b, Some(LIMIT), 0, 0).await;
    let db_a = harness.api_with_pool(POOL).await;
    let db_b = harness.api_with_pool(2).await;
    let probe = WaitProbe::start(harness.db.admin().await, B_LABEL);
    let sampler = RowSampler::start(harness.fixture_conn().await, a);

    // Act
    let rounds_of_b = async {
        for _ in 0..B_ROUNDS {
            db_b.run(Scope::Account(b), async |tx| {
                label_session(tx, B_LABEL).await;
                let ledger = ledger(LIMIT);
                ledger.reserve(tx, b, MIB).await.expect("reserve in B");
                ledger.adjust_used(tx, b, MIB).await.expect("adjust in B");
                ledger
                    .release_reservation(tx, b, MIB)
                    .await
                    .expect("release in B");
                Ok::<_, DbError>(())
            })
            .await
            .expect("round of B");
        }
    };
    let (report, ()) = tokio::join!(workload::run(&db_a, a, 0), rounds_of_b);
    let (longest_wait_ms, probes) = probe.stop().await;
    let samples = sampler.stop().await;

    // Assert
    assert_clean(&report);
    assert!(report.refused > 0, "{report:?}");
    assert_eq!(
        quota_row(&mut conn, a).await,
        (Some(LIMIT), report.ledger_sum, 0)
    );
    assert_eq!(
        quota_row(&mut conn, b).await,
        (Some(LIMIT), B_ROUNDS * MIB, 0)
    );
    assert_never_above_limit(&samples);
    assert!(probes > 0);
    assert!(
        longest_wait_ms <= LOCK_WAIT_BOUND_MS,
        "{longest_wait_ms} ms"
    );
    conn.close().await.expect("close fixture connection");
    drop((db_a, db_b));
    harness.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lock_wait_probe_reports_a_reserve_blocked_by_an_open_reservation() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, Some(LIMIT), 0, 0).await;
    let db = harness.api_with_pool(4).await;
    let mut admin_conn = harness.db.admin().await;
    let (held_tx, held_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel::<()>();
    let holder = {
        let db = db.clone();
        tokio::spawn(async move {
            db.run(Scope::Account(account), async |tx| {
                ledger(LIMIT)
                    .reserve(tx, account, MIB)
                    .await
                    .expect("reserve in the holder");
                held_tx.send(()).expect("test is waiting");
                finish_rx.await.expect("test releases the holder");
                Ok::<_, DbError>(())
            })
            .await
        })
    };
    held_rx.await.expect("holder reserved");
    let blocked = {
        let db = db.clone();
        tokio::spawn(async move {
            db.run(Scope::Account(account), async |tx| {
                label_session(tx, B_LABEL).await;
                ledger(LIMIT)
                    .reserve(tx, account, MIB)
                    .await
                    .expect("reserve in the waiter");
                Ok::<_, DbError>(())
            })
            .await
        })
    };

    // Act
    let waits = loop {
        let waits = lock_waits_ms(&mut admin_conn, B_LABEL).await;
        if !waits.is_empty() {
            break waits;
        }
        tokio::task::yield_now().await;
    };
    finish_tx.send(()).expect("holder is waiting");
    holder.await.expect("holder task").expect("holder commits");
    blocked.await.expect("waiter task").expect("waiter commits");

    // Assert
    assert_eq!(waits.len(), 1);
    assert!(waits[0] >= 0.0);
    assert_eq!(
        quota_row(&mut conn, account).await,
        (Some(LIMIT), 0, 2 * MIB)
    );
    conn.close().await.expect("close fixture connection");
    admin_conn.close().await.expect("close admin connection");
    drop(db);
    harness.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn savepoint_rolled_back_entries_leave_no_trace_in_the_workload() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    account_with_quota(&mut conn, account, Some(LIMIT), 0, 0).await;
    let db = harness.api_with_pool(POOL).await;
    let sampler = RowSampler::start(harness.fixture_conn().await, account);

    // Act
    let report = workload::run(&db, account, ROLLBACK_PERCENT).await;
    let samples = sampler.stop().await;

    // Assert
    assert_clean(&report);
    assert!(report.rolled_back > 0, "{report:?}");
    assert_eq!(
        quota_row(&mut conn, account).await,
        (Some(LIMIT), report.ledger_sum, 0)
    );
    assert_never_above_limit(&samples);
    conn.close().await.expect("close fixture connection");
    drop(db);
    harness.finish().await;
}
