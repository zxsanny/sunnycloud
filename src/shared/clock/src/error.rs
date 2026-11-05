use thiserror::Error;

/// A sleep ended because the clock's shutdown token was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("clock sleep cancelled by shutdown")]
pub struct Cancelled;

/// Failure of the test-clock offset (feature `test-clock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ClockError {
    /// The offset only grows; a negative advance is refused and the offset stays as it was.
    #[error("clock offset cannot go backwards")]
    NegativeAdvance,
    /// The advance does not fit the offset (about 584 years); the offset stays as it was.
    #[error("clock offset overflow")]
    OffsetOverflow,
}
