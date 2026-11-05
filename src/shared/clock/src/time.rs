use std::time::Duration;

/// A UTC instant with microsecond precision; maps to PostgreSQL `timestamptz`.
///
/// Stored as microseconds since 1970-01-01T00:00:00Z (negative before it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const fn from_unix_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn as_unix_micros(self) -> i64 {
        self.0
    }

    /// Adds `by`, truncated to whole microseconds; `None` on overflow.
    pub fn checked_add(self, by: Duration) -> Option<Self> {
        self.0.checked_add(duration_micros(by)?).map(Self)
    }

    /// Subtracts `by`, truncated to whole microseconds; `None` on overflow.
    pub fn checked_sub(self, by: Duration) -> Option<Self> {
        self.0.checked_sub(duration_micros(by)?).map(Self)
    }

    /// Time from `earlier` to `self`; `None` when `earlier` is later than `self`.
    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        let micros = u64::try_from(i128::from(self.0) - i128::from(earlier.0)).ok()?;
        Some(Duration::from_micros(micros))
    }
}

fn duration_micros(by: Duration) -> Option<i64> {
    i64::try_from(by.as_micros()).ok()
}

/// A monotonic reading, measured from the origin of the clock that produced it.
///
/// Opaque: compare it, add a [`Duration`] to it or measure the distance between two readings of the
/// same clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonotonicInstant(Duration);

impl MonotonicInstant {
    pub(crate) const fn from_origin(since_origin: Duration) -> Self {
        Self(since_origin)
    }

    pub(crate) const fn since_origin(self) -> Duration {
        self.0
    }

    /// Adds `by`; `None` on overflow.
    pub fn checked_add(self, by: Duration) -> Option<Self> {
        self.0.checked_add(by).map(Self)
    }

    /// Time from `earlier` to `self`, zero when `earlier` is later.
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}
