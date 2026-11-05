use std::sync::atomic::{AtomicU64, Ordering};

use crate::table::{LIMIT_COUNT, LimitId, Verdict};

/// Counts of results for one limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LimitCounts {
    pub limited: u64,
    pub tripped: u64,
}

/// Snapshot for `auth_rate_limited_total{limit}`: every `Limited` and `Tripped` result since start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LimiterStats {
    per_limit: [LimitCounts; LIMIT_COUNT],
}

impl LimiterStats {
    pub fn get(&self, limit: LimitId) -> LimitCounts {
        self.per_limit[limit.index()]
    }

    pub fn iter(&self) -> impl Iterator<Item = (LimitId, LimitCounts)> + '_ {
        LimitId::ALL
            .into_iter()
            .map(|limit| (limit, self.get(limit)))
    }
}

#[derive(Debug, Default)]
struct Counters {
    limited: AtomicU64,
    tripped: AtomicU64,
}

#[derive(Debug, Default)]
pub(crate) struct StatsCounters {
    per_limit: [Counters; LIMIT_COUNT],
}

impl StatsCounters {
    pub(crate) fn count(&self, limit: LimitId, verdict: Verdict) {
        let counters = &self.per_limit[limit.index()];
        match verdict {
            Verdict::Allowed => {}
            Verdict::Limited { .. } => {
                counters.limited.fetch_add(1, Ordering::Relaxed);
            }
            Verdict::Tripped { .. } => {
                counters.tripped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(crate) fn snapshot(&self) -> LimiterStats {
        let mut snapshot = LimiterStats::default();
        for limit in LimitId::ALL {
            let counters = &self.per_limit[limit.index()];
            snapshot.per_limit[limit.index()] = LimitCounts {
                limited: counters.limited.load(Ordering::Relaxed),
                tripped: counters.tripped.load(Ordering::Relaxed),
            };
        }
        snapshot
    }
}
