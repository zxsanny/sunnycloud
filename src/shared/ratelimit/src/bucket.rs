use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use sc_clock::MonotonicInstant;

use crate::table::{LIMIT_COUNT, LimitId, LimitSpec, Subject, Verdict};

pub(crate) type Key = (LimitId, Subject);

/// A [`LimitSpec`] in exact integer units: one token is `cost` units (the window in nanoseconds) and a
/// nanosecond of refill adds `capacity` units, so refill at `capacity / window` has no rounding drift.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Compiled {
    capacity: u128,
    cost: u128,
}

impl Compiled {
    pub(crate) fn new(spec: LimitSpec) -> Self {
        Self {
            capacity: u128::from(spec.capacity),
            cost: spec.window.as_nanos(),
        }
    }

    fn full(self) -> u128 {
        self.capacity * self.cost
    }

    /// Time until a bucket holding `units` can pay for one failure.
    fn wait(self, units: u128) -> Duration {
        let nanos = self.cost.saturating_sub(units).div_ceil(self.capacity);
        let secs = u64::try_from(nanos / 1_000_000_000).unwrap_or(u64::MAX);
        Duration::new(secs, (nanos % 1_000_000_000) as u32)
    }
}

#[derive(Debug)]
struct Bucket {
    units: u128,
    at: MonotonicInstant,
    seq: u64,
}

impl Bucket {
    fn refilled(&self, now: MonotonicInstant, spec: Compiled) -> u128 {
        let elapsed = now.saturating_duration_since(self.at).as_nanos();
        self.units
            .saturating_add(elapsed.saturating_mul(spec.capacity))
            .min(spec.full())
    }
}

/// One mutex-guarded slice of the bucket map. `order` maps a touch sequence number to the key, so the
/// first entry is always the oldest idle bucket.
#[derive(Debug)]
pub(crate) struct Shard {
    buckets: HashMap<Key, Bucket>,
    order: BTreeMap<u64, Key>,
    next_seq: u64,
    cap: usize,
}

impl Shard {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            buckets: HashMap::new(),
            order: BTreeMap::new(),
            next_seq: 0,
            cap,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.buckets.len()
    }

    /// Read only: a missing bucket is a full one.
    pub(crate) fn check(
        &self,
        key: Key,
        now: MonotonicInstant,
        specs: &[Compiled; LIMIT_COUNT],
    ) -> Verdict {
        let spec = specs[key.0.index()];
        let units = self.current(&key, now, spec);
        if units >= spec.cost {
            Verdict::Allowed
        } else {
            Verdict::Limited {
                retry_after: spec.wait(units),
            }
        }
    }

    /// Takes one token. The call that takes the last one is `Tripped`; calls on an empty bucket are
    /// `Limited` and take nothing.
    pub(crate) fn record_failure(
        &mut self,
        key: Key,
        now: MonotonicInstant,
        specs: &[Compiled; LIMIT_COUNT],
    ) -> Verdict {
        let spec = specs[key.0.index()];
        let units = self.current(&key, now, spec);
        let (left, verdict) = if units >= spec.cost {
            let left = units - spec.cost;
            let verdict = if left < spec.cost {
                Verdict::Tripped {
                    retry_after: spec.wait(left),
                }
            } else {
                Verdict::Allowed
            };
            (left, verdict)
        } else {
            (
                units,
                Verdict::Limited {
                    retry_after: spec.wait(units),
                },
            )
        };
        self.store(key, left, now, specs);
        verdict
    }

    pub(crate) fn clear(&mut self, key: &Key) {
        if let Some(bucket) = self.buckets.remove(key) {
            self.order.remove(&bucket.seq);
        }
    }

    fn current(&self, key: &Key, now: MonotonicInstant, spec: Compiled) -> u128 {
        self.buckets
            .get(key)
            .map_or_else(|| spec.full(), |bucket| bucket.refilled(now, spec))
    }

    fn store(
        &mut self,
        key: Key,
        units: u128,
        now: MonotonicInstant,
        specs: &[Compiled; LIMIT_COUNT],
    ) {
        self.clear(&key);
        self.make_room(now, specs);
        let seq = self.next_seq;
        self.next_seq += 1;
        self.order.insert(seq, key);
        self.buckets.insert(
            key,
            Bucket {
                units,
                at: now,
                seq,
            },
        );
    }

    /// Before a new bucket goes in: drops the oldest buckets that have refilled to capacity, then evicts
    /// the oldest idle ones until one slot is free.
    fn make_room(&mut self, now: MonotonicInstant, specs: &[Compiled; LIMIT_COUNT]) {
        while let Some((&seq, &key)) = self.order.first_key_value() {
            let refilled = self.buckets.get(&key).is_none_or(|bucket| {
                let spec = specs[key.0.index()];
                bucket.refilled(now, spec) >= spec.full()
            });
            if self.buckets.len() < self.cap && !refilled {
                return;
            }
            self.order.remove(&seq);
            self.buckets.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use sc_clock::{Clock, ManualClock, Timestamp};

    use super::*;

    fn specs() -> [Compiled; LIMIT_COUNT] {
        [Compiled::new(LimitSpec::new(10, Duration::from_secs(60))); LIMIT_COUNT]
    }

    fn ip_key(n: u32) -> Key {
        (
            LimitId::SigninIp,
            Subject::Ip(IpAddr::V4(Ipv4Addr::from(n))),
        )
    }

    #[test]
    fn eviction_at_cap_removes_the_oldest_idle_buckets_first() {
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let specs = specs();
        let mut shard = Shard::new(1000);

        for n in 0..2000 {
            shard.record_failure(ip_key(n), clock.monotonic(), &specs);
            assert!(shard.len() <= 1000);
        }

        assert_eq!(shard.len(), 1000);
        for n in 0..1000 {
            assert!(!shard.buckets.contains_key(&ip_key(n)), "old {n} survived");
        }
        for n in 1000..2000 {
            assert!(shard.buckets.contains_key(&ip_key(n)), "new {n} evicted");
        }
    }

    #[test]
    fn a_touched_bucket_is_no_longer_the_oldest() {
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let specs = specs();
        let mut shard = Shard::new(3);
        for n in 0..3 {
            shard.record_failure(ip_key(n), clock.monotonic(), &specs);
        }

        shard.record_failure(ip_key(0), clock.monotonic(), &specs);
        shard.record_failure(ip_key(3), clock.monotonic(), &specs);

        assert!(shard.buckets.contains_key(&ip_key(0)));
        assert!(!shard.buckets.contains_key(&ip_key(1)));
    }

    #[test]
    fn refilled_buckets_are_dropped_when_a_new_one_arrives() {
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let specs = specs();
        let mut shard = Shard::new(1000);
        for n in 0..10 {
            shard.record_failure(ip_key(n), clock.monotonic(), &specs);
        }

        clock.tick(Duration::from_secs(60));
        shard.record_failure(ip_key(100), clock.monotonic(), &specs);

        assert_eq!(shard.len(), 1);
        assert_eq!(shard.order.len(), 1);
    }

    #[test]
    fn the_order_index_matches_the_bucket_map() {
        let clock = ManualClock::new(Timestamp::from_unix_micros(0));
        let specs = specs();
        let mut shard = Shard::new(5);
        for n in 0..50 {
            shard.record_failure(ip_key(n % 9), clock.monotonic(), &specs);
            clock.tick(Duration::from_secs(1));
        }
        shard.clear(&ip_key(8));

        assert_eq!(shard.order.len(), shard.buckets.len());
        assert!(shard.buckets.len() <= 5);
    }
}
