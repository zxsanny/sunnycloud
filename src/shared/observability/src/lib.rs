//! Logging, metrics and the ops router every sunnycloud process shares.
//!
//! Contract: `_docs/02_document/contracts/shared_observability/sc_observability.md`.
//!
//! - [`init_logging`] installs the one JSON (or text) line format; the typed field API and the formatter keep
//!   secrets, tokens, names and paths out of it (see [`logging`]).
//! - [`Metrics`] is the registry. Every family is exported as `sunnycloud_<name>`; constructors take a
//!   [`LabelSet`], a static enumeration, so a label value cannot come from a request. `db_collector`,
//!   `audit_collector` (feature `db`, off in the worker and the decoder so they link no `sqlx`) and
//!   [`ratelimit_collector`] export the helper-owned families by reading the helpers' snapshots, so `sc_db`,
//!   `sc_audit` and `sc_ratelimit` never depend on this crate.
//! - [`ops_router`] serves `/healthz` (checks nothing), `/readyz` (every registered [`ReadinessCheck`] with a
//!   timeout each, 503 on any not-ok check and after [`Liveness::shut_down`]) and `/metrics`.
//! - [`HealthContributor`] and [`ReadinessCheck`] are declared here and re-exported by `contracts`; this crate
//!   never imports `contracts` (owner decision 2026-11-05).
//!
//! # Metrics library
//!
//! `prometheus` 0.14 (Apache-2.0) with an explicit [`Registry`]; no metrics library existed in the workspace,
//! so there was no precedent to follow. Chosen over `prometheus-client` because its registry can prefix every
//! gathered family once (`Registry::new_custom`, which is how "prefixed exactly once" holds for the process
//! collector and for collectors written elsewhere), it ships the standard process collector, and its text
//! encoder is the reference one. `default-features = false` drops the protobuf encoder and the `protobuf`
//! crate; only the text format is served. Counter increments on a family without labels are one atomic add;
//! with labels they also take a read lock on the child map.
//!
//! # Adapting 07's `HealthContributor` to a `ReadinessCheck`
//!
//! The admin health view (07) aggregates [`HealthContributor`]s (`Ok | Degraded | Down`); `/readyz` runs
//! [`ReadinessCheck`]s (`Ok | Degraded | Failing`). A contributor becomes a check by mapping
//! `Down` to `Failing` and keeping `Ok` and `Degraded`; `/readyz` answers 503 for both `Degraded` and
//! `Failing`, which is the conservative reading (E-09 risk 4 is open with the owner for 04's suspect blobs).
//! The adapter is a few lines in the composition root, not a type here, because only 07 owns contributors.

mod error;
mod health;
pub mod logging;
mod metrics;
mod ops;

pub use error::ObsError;
pub use health::{
    CheckFuture, HealthContributor, HealthItem, HealthStatus, ReadinessCheck, ReadinessItem,
    ReadinessStatus,
};
pub use logging::{
    Component, LineSink, LogConfig, LogFormat, LogGuard, LogStats, field, init_logging,
    request_span, subscriber,
};
pub use metrics::{
    BuildInfo, Counter, Gauge, HISTOGRAM_BUCKETS, Histogram, LabelSet, MAX_LABELS, Metrics,
    NoLabels, PREFIX, ScrapeProbe, SnapshotCollector, ratelimit_collector,
};
#[cfg(feature = "db")]
pub use metrics::{DbCollector, audit_collector, db_collector};
pub use ops::{DEFAULT_CHECK_TIMEOUT, Liveness, ReadinessRegistry, ops_router};
pub use prometheus::Registry;
pub use prometheus::core::Collector;

#[doc(hidden)]
pub mod __private {
    pub use tracing;
}

/// Logs one event at `level` with a literal message and typed fields:
/// `log_event!(Level::INFO, "upload committed", request_id = id, status = 201)`.
/// The field names are those of the schema (see [`logging::field`]); any other name does not compile.
#[macro_export]
macro_rules! log_event {
    ($level:expr, $msg:literal $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::__private::tracing::event!($level, $( $key = $crate::field::$key($value), )* $msg)
    };
}

#[macro_export]
macro_rules! log_error {
    ($($args:tt)+) => { $crate::log_event!($crate::__private::tracing::Level::ERROR, $($args)+) };
}

#[macro_export]
macro_rules! log_warn {
    ($($args:tt)+) => { $crate::log_event!($crate::__private::tracing::Level::WARN, $($args)+) };
}

#[macro_export]
macro_rules! log_info {
    ($($args:tt)+) => { $crate::log_event!($crate::__private::tracing::Level::INFO, $($args)+) };
}

#[macro_export]
macro_rules! log_debug {
    ($($args:tt)+) => { $crate::log_event!($crate::__private::tracing::Level::DEBUG, $($args)+) };
}
