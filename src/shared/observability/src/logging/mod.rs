//! The one logging setup: single-line JSON (or text) with a fixed field schema.
//!
//! # Field schema
//!
//! `ts` (UTC RFC 3339), `level`, `target`, `msg`, `request_id` (`null` outside a request), `component`, and
//! when set `account_id`, `node_id`, `route`, `status`, `duration_ms`, `err`. Events of target `audit` also
//! carry `"stream":"audit"` and the audit fields `sc_audit` mirrors (`event`, `actor_account_id`,
//! `actor_session_id`, `subject_type`, `subject_id`, `ip`, `detail`).
//!
//! # Redaction
//!
//! Two layers, both closed by default:
//!
//! 1. The typed API ([`log_error!`](crate::log_error) and the other level macros, the [`field`] functions):
//!    `msg` is a literal, and every field value goes through a function that accepts one type
//!    (`CorrelationId`, `Uuid`, `&'static str` route, `u16` status, `u64` milliseconds, `&dyn Error`). A free
//!    `String`, a token, a path or an email has no slot. A field name outside the schema does not compile.
//! 2. The formatter renders only the schema. Any other field of a raw `tracing` event is dropped, an id that
//!    is not a UUID or a status outside 100..=599 is dropped, and each drop is counted
//!    ([`LogStats::rejected_fields`]). Spans carry the request context (`request_id`, `account_id`,
//!    `node_id`, `route`) to the events inside them; use [`request_span`].
//!
//! ```compile_fail
//! // a field outside the schema
//! sc_observability::log_info!("login", token = "abc");
//! ```
//!
//! ```compile_fail
//! // a free string where an id is expected
//! sc_observability::log_info!("login", request_id = "not-an-id");
//! ```
//!
//! ```compile_fail
//! // a formatted message
//! let path = "/home/alice/a.txt";
//! sc_observability::log_info!(format!("opened {path}"));
//! ```
//!
//! What no type can prevent: the text of a raw `tracing` message and the `Display` of an error passed as
//! `err`. Error types in this workspace carry ids and codes only, and the full-suite grep of NFT-SEC-38 is the
//! backstop for the rest.
//!
//! Lines are written whole by one `write_all`; a failed write is counted ([`LogStats::dropped_lines`]).

mod event;
pub mod field;
mod layer;
mod render;
mod time;

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use sc_clock::{Clock, SystemClock};
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;
use uuid::Uuid;

use crate::error::ObsError;
use event::Counters;
use layer::LogLayer;

pub use event::AUDIT_TARGET;
pub use layer::{LineSink, Stdout};
pub use tracing::Level;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Text,
}

impl FromStr for LogFormat {
    type Err = ObsError;

    /// The values of the `log` config key.
    fn from_str(text: &str) -> Result<Self, ObsError> {
        match text {
            "json" => Ok(LogFormat::Json),
            "text" => Ok(LogFormat::Text),
            other => Err(ObsError::BadConfig(format!(
                "log format `{other}` is not `json` or `text`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Api,
    Worker,
    DecoderRunner,
}

impl Component {
    pub const fn as_str(self) -> &'static str {
        match self {
            Component::Api => "api",
            Component::Worker => "worker",
            Component::DecoderRunner => "decoder-runner",
        }
    }
}

/// `format` is the `log` key, `level` the `log_level` key (parse it with [`Level::from_str`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogConfig {
    pub format: LogFormat,
    pub level: Level,
    pub component: Component,
}

/// What the layer could not do. Both stay zero in a healthy process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogStats {
    /// Lines the sink refused (stdout closed, disk full).
    pub dropped_lines: u64,
    /// Fields dropped for being outside the schema or of the wrong shape.
    pub rejected_fields: u64,
}

/// Returned by [`init_logging`]; keep it for the life of the process. Dropping it flushes stdout.
#[must_use = "dropping the guard flushes stdout; keep it until the process exits"]
pub struct LogGuard {
    counters: Arc<Counters>,
}

impl LogGuard {
    pub fn stats(&self) -> LogStats {
        stats_of(&self.counters)
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        use std::io::Write;
        if std::io::stdout().flush().is_err() {
            self.counters.dropped_lines.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn stats_of(counters: &Counters) -> LogStats {
    LogStats {
        dropped_lines: counters.dropped_lines.load(Ordering::Relaxed),
        rejected_fields: counters.rejected_fields.load(Ordering::Relaxed),
    }
}

/// Installs the subscriber for the process, writing to stdout. `AlreadyInstalled` when a global subscriber
/// exists.
pub fn init_logging(cfg: LogConfig) -> Result<LogGuard, ObsError> {
    let counters = Arc::new(Counters::default());
    let subscriber = build(
        cfg,
        Arc::new(SystemClock::new()),
        Arc::new(Stdout),
        Arc::clone(&counters),
    );
    tracing::subscriber::set_global_default(subscriber).map_err(|_| ObsError::AlreadyInstalled)?;
    Ok(LogGuard { counters })
}

/// The subscriber [`init_logging`] installs, with the clock and the sink injected: `ts` comes from `clock`
/// and finished lines go to `sink`. For tests and for embedding in a process that owns its own subscriber
/// setup; the second value reads the counters.
pub fn subscriber(
    cfg: LogConfig,
    clock: Arc<dyn Clock>,
    sink: Arc<dyn LineSink>,
) -> (
    impl Subscriber + Send + Sync,
    impl Fn() -> LogStats + Send + Sync,
) {
    let counters = Arc::new(Counters::default());
    let subscriber = build(cfg, clock, sink, Arc::clone(&counters));
    (subscriber, move || stats_of(&counters))
}

fn build(
    cfg: LogConfig,
    clock: Arc<dyn Clock>,
    sink: Arc<dyn LineSink>,
    counters: Arc<Counters>,
) -> impl Subscriber + Send + Sync + 'static {
    Registry::default()
        .with(LevelFilter::from_level(cfg.level))
        .with(LogLayer {
            format: cfg.format,
            component: cfg.component,
            clock,
            sink,
            counters,
        })
}

/// A span carrying the request id: every event inside it (on any level the filter lets through) is logged
/// with that `request_id`. It is an error-level span so the filter never disables it.
pub fn request_span(request_id: sc_error::CorrelationId) -> tracing::Span {
    tracing::error_span!("request", request_id = field::request_id(request_id))
}

/// Span for work done on behalf of one account (a janitor cycle, a deletion).
pub fn account_span(account_id: Uuid) -> tracing::Span {
    tracing::error_span!("account", account_id = field::account_id(account_id))
}
