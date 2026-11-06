//! The database and audit collectors (feature `db`): they read the snapshots of `sc_db` and `sc_audit`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use prometheus::core::{Collector, Desc};
use prometheus::proto::MetricFamily;
use sc_audit::AuditStats;
use sc_db::{Db, DbStats};

use super::snapshot::{SnapshotCollector, counter_family, gauge_family, summary_family};
use super::{Metrics, ScrapeProbe};
use crate::error::ObsError;
use crate::health::CheckFuture;

pub(crate) fn db_families(stats: &DbStats, up: bool) -> Vec<MetricFamily> {
    let mut families = vec![
        gauge_family(
            "db_up",
            "1 when the database answered the last probe",
            &[(&[], f64::from(up))],
        ),
        gauge_family(
            "db_pool_connections",
            "connections of the pool by state",
            &[
                (&[("state", "in_use")], f64::from(stats.pool_in_use)),
                (&[("state", "idle")], f64::from(stats.pool_idle)),
                (&[("state", "max")], f64::from(stats.pool_max)),
            ],
        ),
    ];
    if let Some(pending) = stats.migrations_pending {
        #[allow(clippy::cast_precision_loss)]
        let pending = pending as f64;
        families.push(gauge_family(
            "migrations_pending",
            "embedded migrations not yet applied",
            &[(&[], pending)],
        ));
    }
    if !stats.queries.is_empty() {
        let samples: Vec<_> = stats
            .queries
            .iter()
            .map(|query| ([("op", query.op)], query.count, query.total.as_secs_f64()))
            .collect();
        let samples: Vec<_> = samples
            .iter()
            .map(|(labels, count, sum)| (&labels[..], *count, *sum))
            .collect();
        families.push(summary_family(
            "db_query_duration_seconds",
            "units of work by operation: count and total seconds",
            &samples,
        ));
    }
    families
}

pub(crate) fn audit_families(stats: AuditStats) -> Vec<MetricFamily> {
    #[allow(clippy::cast_precision_loss)]
    let failures = stats.write_failures as f64;
    vec![counter_family(
        "audit_write_failures_total",
        "best-effort audit events that did not reach the table",
        &[(&[], failures)],
    )]
}

/// `db_up`, `db_pool_connections{state}`, `db_query_duration_seconds{op}` and `migrations_pending`.
///
/// `db_up` needs a probe, which is async: [`DbCollector::register`] also registers the probe, and
/// [`Metrics::render`] awaits it before each scrape.
#[derive(Clone)]
pub struct DbCollector {
    db: Db,
    up: Arc<AtomicBool>,
    snapshot: Arc<SnapshotCollector>,
}

pub fn db_collector(db: Db) -> DbCollector {
    let up = Arc::new(AtomicBool::new(false));
    let read = {
        let (db, up) = (db.clone(), Arc::clone(&up));
        move || db_families(&db.stats(), up.load(Ordering::Acquire))
    };
    let snapshot = SnapshotCollector::new(
        &[
            ("db_up", "1 when the database answered the last probe", &[]),
            (
                "db_pool_connections",
                "connections of the pool by state",
                &["state"],
            ),
            (
                "migrations_pending",
                "embedded migrations not yet applied",
                &[],
            ),
            (
                "db_query_duration_seconds",
                "units of work by operation",
                &["op"],
            ),
        ],
        read,
    );
    DbCollector {
        db,
        up,
        snapshot: Arc::new(snapshot),
    }
}

impl DbCollector {
    /// Registers the collector and its scrape probe.
    pub fn register(self, metrics: &Metrics) -> Result<(), ObsError> {
        metrics.register_probe(Arc::new(self.clone()));
        metrics.register_collector(Box::new(self))
    }
}

impl Collector for DbCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.snapshot.desc()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        self.snapshot.collect()
    }
}

impl ScrapeProbe for DbCollector {
    fn refresh(&self) -> CheckFuture<'_, ()> {
        Box::pin(async move {
            let health = self.db.health().await;
            self.up.store(health.reachable, Ordering::Release);
        })
    }
}

/// `audit_write_failures_total`, read from `stats`: `move || writer.stats()` with an `AuditWriter` clone.
pub fn audit_collector(
    stats: impl Fn() -> AuditStats + Send + Sync + 'static,
) -> Box<dyn Collector> {
    Box::new(SnapshotCollector::new(
        &[(
            "audit_write_failures_total",
            "best-effort audit events that did not reach the table",
            &[],
        )],
        move || audit_families(stats()),
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use prometheus::{Encoder, TextEncoder};
    use sc_db::QueryStats;

    use super::*;

    fn render(families: &[MetricFamily]) -> String {
        let mut out = Vec::new();
        TextEncoder::new()
            .encode(families, &mut out)
            .expect("encode");
        String::from_utf8(out).expect("utf8")
    }

    fn stats(pending: Option<usize>, queries: Vec<QueryStats>) -> DbStats {
        DbStats {
            pool_in_use: 2,
            pool_idle: 1,
            pool_max: 10,
            queries,
            migrations_pending: pending,
        }
    }

    #[test]
    fn db_families_map_the_snapshot_and_the_probe() {
        let queries = vec![QueryStats {
            op: "run",
            count: 3,
            total: Duration::from_millis(1500),
            max: Duration::from_millis(900),
        }];

        let text = render(&db_families(&stats(Some(2), queries), false));

        assert!(text.contains("db_up 0\n"), "{text}");
        assert!(text.contains("db_pool_connections{state=\"in_use\"} 2"));
        assert!(text.contains("db_pool_connections{state=\"idle\"} 1"));
        assert!(text.contains("db_pool_connections{state=\"max\"} 10"));
        assert!(text.contains("migrations_pending 2\n"));
        assert!(text.contains("db_query_duration_seconds_count{op=\"run\"} 3"));
        assert!(text.contains("db_query_duration_seconds_sum{op=\"run\"} 1.5"));
    }

    #[test]
    fn unknown_pending_and_no_queries_export_no_series_instead_of_a_false_zero() {
        let text = render(&db_families(&stats(None, vec![]), true));

        assert!(text.contains("db_up 1\n"));
        assert!(!text.contains("migrations_pending"));
        assert!(!text.contains("db_query_duration_seconds"));
    }

    #[test]
    fn audit_failures_are_a_counter() {
        let text = render(&audit_families(AuditStats { write_failures: 7 }));

        assert!(text.contains("# TYPE audit_write_failures_total counter"));
        assert!(text.contains("audit_write_failures_total 7\n"));
    }
}
