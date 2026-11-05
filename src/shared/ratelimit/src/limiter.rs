use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::BuildHasher;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sc_clock::Clock;

use crate::bucket::{Compiled, Key, Shard};
use crate::stats::{LimiterStats, StatsCounters};
use crate::table::{ConfigError, LIMIT_COUNT, LimitId, LimitTable, Subject, Verdict};

const MAX_SHARDS: usize = 16;

/// In-memory token-bucket limiter for the AC-20 abuse failures. Build it once in the composition root
/// and share it by `Arc`; there is no global.
pub struct AbuseLimiter {
    clock: Arc<dyn Clock>,
    specs: [Compiled; LIMIT_COUNT],
    shards: Vec<Mutex<Shard>>,
    hasher: RandomState,
    stats: StatsCounters,
}

impl AbuseLimiter {
    /// Rejects a zero capacity, a zero window or a zero bucket cap.
    ///
    /// The cap is split evenly over the shards (`max_entries / shards`, rounded down), so the map never
    /// holds more than `max_entries` buckets. The shard hash is keyed per limiter, so an attacker cannot
    /// aim addresses at one shard.
    pub fn new(clock: Arc<dyn Clock>, table: LimitTable) -> Result<Self, ConfigError> {
        table.validate()?;
        let shard_count = table.max_entries().min(MAX_SHARDS);
        let per_shard = table.max_entries() / shard_count;
        Ok(Self {
            clock,
            specs: LimitId::ALL.map(|limit| Compiled::new(table.spec(limit))),
            shards: (0..shard_count)
                .map(|_| Mutex::new(Shard::new(per_shard)))
                .collect(),
            hasher: RandomState::new(),
            stats: StatsCounters::default(),
        })
    }

    /// Read only: takes nothing from the bucket.
    pub fn check(&self, limit: LimitId, subject: &Subject) -> Verdict {
        let key = (limit, *subject);
        let now = self.clock.monotonic();
        let verdict = self.shard(&key).check(key, now, &self.specs);
        self.stats.count(limit, verdict);
        verdict
    }

    /// Takes one token after a failure; see [`Verdict`].
    pub fn record_failure(&self, limit: LimitId, subject: &Subject) -> Verdict {
        let key = (limit, *subject);
        let now = self.clock.monotonic();
        let verdict = self.shard(&key).record_failure(key, now, &self.specs);
        self.stats.count(limit, verdict);
        verdict
    }

    /// Forgets the subject's failures (after a success where consecutive failures count).
    pub fn clear(&self, limit: LimitId, subject: &Subject) {
        let key = (limit, *subject);
        self.shard(&key).clear(&key);
    }

    pub fn stats(&self) -> LimiterStats {
        self.stats.snapshot()
    }

    /// Buckets currently held; never above `ratelimit_max_entries`.
    pub fn bucket_count(&self) -> usize {
        self.shards.iter().map(|shard| lock(shard).len()).sum()
    }

    fn shard(&self, key: &Key) -> MutexGuard<'_, Shard> {
        let index = self.hasher.hash_one(key) as usize % self.shards.len();
        lock(&self.shards[index])
    }
}

/// Nothing inside a shard's critical section panics, so a poisoned lock still guards consistent data.
fn lock(shard: &Mutex<Shard>) -> MutexGuard<'_, Shard> {
    shard.lock().unwrap_or_else(PoisonError::into_inner)
}

impl fmt::Debug for AbuseLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbuseLimiter")
            .field("shards", &self.shards.len())
            .finish_non_exhaustive()
    }
}
