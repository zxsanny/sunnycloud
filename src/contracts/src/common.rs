//! Types shared by the ports of every component.

use std::error::Error;

/// Payload of the tier-3 `Unexpected` variant of every port error: a bug or an infrastructure fault.
/// It is for the log only; the delivery layer turns it into an opaque response (`sc_error::DomainError`).
pub type Fault = Box<dyn Error + Send + Sync>;

/// Offset paging of a list port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub offset: u64,
    pub limit: u32,
}
