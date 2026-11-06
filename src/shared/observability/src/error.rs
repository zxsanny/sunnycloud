/// Failures of the observability setup. None of them can occur on a hot path: every one is returned by a
/// constructor or by `init_logging` while a process starts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ObsError {
    #[error("a global tracing subscriber is already installed")]
    AlreadyInstalled,
    #[error("label set of metric `{metric}` refused: {reason}")]
    BadLabel {
        metric: &'static str,
        reason: String,
    },
    #[error(
        "metric name `{0}` refused: lower case letters, digits and `_`, no `sunnycloud_` prefix"
    )]
    BadName(String),
    #[error("`{0}` is already registered")]
    Duplicate(String),
    #[error("{0}")]
    BadConfig(String),
    #[error("the registry refused the metric: {0}")]
    Registry(String),
    #[error("encoding the metrics failed: {0}")]
    Encode(String),
}
