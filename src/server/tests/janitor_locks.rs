//! AC-4: the scheduler takes no leader lock. A real janitor cycles against PostgreSQL under the real
//! scheduler while `pg_locks` is sampled. The full `serve` stack (IT-12-9) is not built yet, so the janitor is
//! the audit retention janitor and the API process is this test process.

#[path = "../../shared/audit/tests/support/mod.rs"]
mod support;

mod janitor_support;

use std::sync::Arc;
use std::time::Duration;

use janitor_support::{metrics, series, system_clock};
use sunnycloud_server::janitor::{AuditRetentionJanitor, JanitorMetrics, Scheduled, Scheduler};
use support::Fixture;

const SAMPLES: usize = 100;

#[tokio::test]
async fn no_advisory_lock_is_held_while_a_database_janitor_cycles() {
    // Arrange
    let fx = Fixture::new().await;
    let metrics = metrics();
    let janitor_metrics = Arc::new(JanitorMetrics::new(&metrics).expect("janitor families"));
    let scheduler = Scheduler::start(
        system_clock(),
        janitor_metrics,
        vec![Scheduled {
            janitor: Arc::new(AuditRetentionJanitor::new(
                fx.api.clone(),
                Duration::from_secs(86_400),
            )),
            interval: Duration::from_millis(5),
        }],
    )
    .expect("scheduler starts");
    let held = "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' \
                AND database = (SELECT oid FROM pg_database WHERE datname = current_database())";

    // Act
    let mut samples = Vec::new();
    for _ in 0..SAMPLES {
        samples.push(fx.db.admin_count(held).await);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    scheduler.stop().await;
    let text = metrics.render().await.expect("metrics render");

    // Assert
    let cycles = series(
        &text,
        "sunnycloud_janitor_cycles_total",
        &[("task", "audit_retention"), ("result", "ok")],
    )
    .expect("the janitor cycled");
    assert!(cycles >= 10.0, "only {cycles} cycles ran during sampling");
    assert_eq!(samples, vec![0; SAMPLES]);
    fx.db.drop_db().await;
}
