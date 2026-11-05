//! Injectable time for every sunnycloud crate.
//!
//! No component reads the system clock directly. Wall time ([`Clock::now_utc`]), monotonic time
//! ([`Clock::monotonic`]) and cancellable sleeps ([`Clock::sleep_until_monotonic`]) come through the
//! [`Clock`] trait, so retention (7 days), the tombstone horizon (90 days) and share expiry are tested by
//! advancing a clock, and client timing rules (the 2 s settle) run without sleeping.
//!
//! # Which clock each layer uses
//!
//! | Layer | Clock |
//! |---|---|
//! | Release API, worker, desktop daemon, mobile | [`SystemClock`] |
//! | `-test` API and worker images (feature `test-clock`) | `OffsetClock`: system time plus an atomic, non-negative, grow-only offset on wall time only |
//! | Unit and component tests | [`ManualClock`]: both clocks move only on [`ManualClock::tick`] |
//!
//! # Rules
//!
//! * `monotonic` is never offset, so advancing the offset never wakes a sleeper early.
//! * SQL never calls `now()`; the application binds a [`Timestamp`] (`scripts/ci/check-no-sql-now.sh`).
//! * Ordering is decided by `account_feeds.seq`, never by a timestamp.
//! * The `test-clock` feature must never reach a release binary (`scripts/ci/check-release-symbols.sh`).
//! * [`Clock::now_utc`] and [`Clock::monotonic`] are lock-free reads that do not allocate.
//! * A sleep ends with [`Cancelled`] when the clock's shutdown token is cancelled
//!   (`new()` creates a private token, `with_shutdown` shares the application's token).

mod error;
mod manual;
#[cfg(feature = "test-clock")]
mod offset;
mod system;
mod time;
mod wake;

use std::future::Future;
use std::pin::Pin;

pub use error::{Cancelled, ClockError};
pub use manual::ManualClock;
#[cfg(feature = "test-clock")]
pub use offset::OffsetClock;
pub use system::SystemClock;
pub use time::{MonotonicInstant, Timestamp};
pub use tokio_util::sync::CancellationToken;
pub use wake::wake_gap_exceeded;

/// Future returned by [`Clock::sleep_until_monotonic`]; boxed so a `dyn Clock` can be injected.
pub type SleepFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Cancelled>> + Send + 'a>>;

/// The one source of time.
pub trait Clock: Send + Sync {
    /// Current UTC time, microsecond precision. Lock-free, no allocation.
    fn now_utc(&self) -> Timestamp;

    /// Current monotonic reading; never goes back. Lock-free, no allocation. Readings are comparable
    /// only with readings of the same clock.
    fn monotonic(&self) -> MonotonicInstant;

    /// Completes at the monotonic instant `at` (immediately when it has passed), or returns
    /// [`Cancelled`] when the clock's shutdown token is cancelled.
    fn sleep_until_monotonic(&self, at: MonotonicInstant) -> SleepFuture<'_>;
}
