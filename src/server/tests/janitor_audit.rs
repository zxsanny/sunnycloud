//! AC-6: the audit retention janitor removes only rows older than `now - audit_retention`, through the purge
//! function, against real PostgreSQL. Provisioning is the `sc_testdb` harness; the rows go in through the real
//! `sc_audit` writer.

use std::sync::Arc;
use std::time::Duration;

use contracts::{Janitor, JanitorError};
use sc_audit::{AuditEvent, AuditEventKind, AuditWriter};
use sc_clock::{ManualClock, Timestamp};
use sc_db::Db;
use sc_testdb::{START, TestDb};
use sunnycloud_server::janitor::AuditRetentionJanitor;

const DAY: Duration = Duration::from_secs(86_400);
const RETENTION: Duration = Duration::from_secs(365 * 86_400);

struct Fixture {
    db: TestDb,
    api: Db,
}

impl Fixture {
    async fn new() -> Fixture {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        Fixture { db, api }
    }
}

fn days_before_start(days: u32) -> Timestamp {
    START.checked_sub(DAY * days).expect("in range")
}

async fn seed(fx: &Fixture, days: u32, count: usize) {
    let writer = AuditWriter::new(
        fx.api.clone(),
        Arc::new(ManualClock::new(days_before_start(days))),
    );
    for _ in 0..count {
        writer
            .record(AuditEvent::new(AuditEventKind::SignIn))
            .await
            .expect("seed row");
    }
}

/// The age in whole days of each remaining row, relative to [`START`].
async fn remaining_ages(fx: &Fixture) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT (($1::bigint - EXTRACT(EPOCH FROM at) * 1000000) / 86400000000)::bigint \
         FROM audit_log ORDER BY id",
    )
    .bind(START.as_unix_micros())
    .fetch_all(&mut fx.db.admin().await)
    .await
    .expect("remaining rows")
}

async fn seed_around_the_cutoff(fx: &Fixture) {
    seed(fx, 400, 2).await;
    seed(fx, 366, 1).await;
    seed(fx, 365, 1).await; // exactly at the cutoff: not older
    seed(fx, 300, 2).await;
    seed(fx, 1, 1).await;
}

#[tokio::test]
async fn cycle_removes_only_rows_older_than_now_minus_retention() {
    // Arrange
    let fx = Fixture::new().await;
    seed_around_the_cutoff(&fx).await;
    let janitor = AuditRetentionJanitor::new(fx.api.clone(), RETENTION);

    // Act
    let first = janitor.run_cycle(START).await.expect("first cycle");
    let second = janitor.run_cycle(START).await.expect("second cycle");

    // Assert
    assert_eq!(janitor.name(), "audit_retention");
    assert_eq!(first.processed, 3);
    assert_eq!(second.processed, 0);
    assert_eq!(remaining_ages(&fx).await, [365, 300, 300, 1]);
    fx.db.drop_db().await;
}

#[tokio::test]
async fn retention_zero_removes_no_row_and_runs_no_statement() {
    // Arrange
    let fx = Fixture::new().await;
    seed_around_the_cutoff(&fx).await;
    // With the function gone a purge that ran would fail; with retention 0 it must not run at all.
    fx.db
        .admin_exec("DROP FUNCTION audit_log_purge(timestamptz)")
        .await;
    let janitor = AuditRetentionJanitor::new(fx.api.clone(), Duration::ZERO);

    // Act
    let stats = janitor.run_cycle(START).await.expect("cycle");

    // Assert
    assert_eq!(stats.processed, 0);
    assert_eq!(remaining_ages(&fx).await.len(), 7);
    fx.db.drop_db().await;
}

#[tokio::test]
async fn a_store_failure_is_returned_as_an_unexpected_janitor_error() {
    // Arrange
    let fx = Fixture::new().await;
    fx.db
        .admin_exec("DROP FUNCTION audit_log_purge(timestamptz)")
        .await;
    let janitor = AuditRetentionJanitor::new(fx.api.clone(), RETENTION);

    // Act
    let result = janitor.run_cycle(START).await;

    // Assert
    assert!(matches!(result, Err(JanitorError::Unexpected(_))));
    fx.db.drop_db().await;
}

#[tokio::test]
async fn a_retention_beyond_the_clock_range_removes_nothing() {
    // Arrange
    let fx = Fixture::new().await;
    seed(&fx, 400, 1).await;
    let janitor = AuditRetentionJanitor::new(fx.api.clone(), Duration::from_secs(u64::MAX / 2));

    // Act
    let stats = janitor.run_cycle(START).await.expect("cycle");

    // Assert
    assert_eq!(stats.processed, 0);
    assert_eq!(remaining_ages(&fx).await.len(), 1);
    fx.db.drop_db().await;
}
