//! Collectors for the helper-owned families that need no database. Each reads the read-only snapshot of its
//! helper at scrape time. The database and audit ones are in `db` (feature `db`).

use prometheus::core::Collector;
use prometheus::proto::MetricFamily;
use sc_ratelimit::LimiterStats;

use super::snapshot::{SnapshotCollector, counter_family};

pub(crate) fn ratelimit_families(stats: &LimiterStats) -> Vec<MetricFamily> {
    let samples: Vec<_> = stats
        .iter()
        .map(|(limit, counts)| {
            #[allow(clippy::cast_precision_loss)]
            let total = (counts.limited + counts.tripped) as f64;
            ([("limit", limit.label())], total)
        })
        .collect();
    let samples: Vec<_> = samples
        .iter()
        .map(|(labels, total)| (&labels[..], *total))
        .collect();
    vec![counter_family(
        "auth_rate_limited_total",
        "abuse limiter results that limited or tripped, by limit",
        &samples,
    )]
}

/// `auth_rate_limited_total{limit}`, read from `stats`: `move || limiter.stats()` with an `Arc<AbuseLimiter>`.
pub fn ratelimit_collector(
    stats: impl Fn() -> LimiterStats + Send + Sync + 'static,
) -> Box<dyn Collector> {
    Box::new(SnapshotCollector::new(
        &[(
            "auth_rate_limited_total",
            "abuse limiter results that limited or tripped, by limit",
            &["limit"],
        )],
        move || ratelimit_families(&stats()),
    ))
}
