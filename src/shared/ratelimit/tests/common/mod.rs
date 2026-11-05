use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use sc_clock::{ManualClock, Timestamp};
use sc_ratelimit::{AbuseLimiter, LimitSpec, LimitTable, Subject};
use uuid::Uuid;

/// Numbers for the two limits that have no AC-20 default; tests pick their own.
pub fn table() -> LimitTable {
    LimitTable::new(
        LimitSpec::new(30, Duration::from_secs(60)),
        LimitSpec::new(40, Duration::from_secs(60)),
    )
}

pub fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Timestamp::from_unix_micros(0)))
}

pub fn limiter(clock: &Arc<ManualClock>, table: LimitTable) -> AbuseLimiter {
    AbuseLimiter::new(clock.clone(), table).expect("valid table")
}

pub fn ip(n: u32) -> Subject {
    Subject::Ip(IpAddr::V4(Ipv4Addr::from(n)))
}

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
