use sc_clock::Clock;
use uuid::{Builder, Uuid};

/// A fresh UUIDv7: millisecond time from `clock`, random tail from the OS or browser CSPRNG.
///
/// Ids from one clock sort by creation time at millisecond resolution; ids inside one millisecond are
/// unordered. A timestamp before 1970 is clamped to the epoch.
///
/// # Panics
///
/// When the platform has no secure random source. Id generation has no error path in the contract, and a
/// guessable fallback would break uniqueness silently.
pub fn new_id(clock: &dyn Clock) -> Uuid {
    let millis = u64::try_from(clock.now_utc().as_unix_micros() / 1000).unwrap_or(0);
    let mut tail = [0u8; 10];
    if let Err(error) = getrandom::fill(&mut tail) {
        panic!("new_id: secure random source unavailable: {error}");
    }
    Builder::from_unix_timestamp_millis(millis, &tail).into_uuid()
}
