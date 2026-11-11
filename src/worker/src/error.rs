use std::io;

use sc_config::ConfigError;
use sc_observability::ObsError;
use thiserror::Error;

use crate::api::ApiConfigError;
use crate::decoder_client::BackoffError;
use crate::probe::ProbeError;
use crate::scratch::ScratchError;

/// Why the worker did not start or why a subcommand failed. No variant carries a setting value.
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("worker_ops_listen is not a listen address (`host:port` or `:port`)")]
    OpsListen,
    #[error("nvenc must be `on` or `off`")]
    Nvenc,
    #[error("{0} is not set")]
    Unset(&'static str),
    #[error("the API client cannot be built: {0}")]
    Api(#[from] ApiConfigError),
    #[error(transparent)]
    Backoff(#[from] BackoffError),
    #[error("scratch cleanup failed: {0}")]
    Scratch(#[from] ScratchError),
    #[error("observability setup failed: {0}")]
    Observability(#[from] ObsError),
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        source: io::Error,
    },
    #[error("unknown argument `{0}`; usage: sunnycloud-worker [healthcheck]")]
    Usage(String),
    #[error("ops listener is not ready: {0}")]
    Unhealthy(#[from] ProbeError),
}

impl WorkerError {
    pub(crate) fn io(context: &'static str, source: io::Error) -> Self {
        WorkerError::Io { context, source }
    }

    /// 1 unhealthy (Docker's "unhealthy" code), 2 runtime failure, 3 configuration or usage.
    pub fn exit_code(&self) -> u8 {
        match self {
            WorkerError::Unhealthy(_) => 1,
            WorkerError::Observability(_) | WorkerError::Io { .. } | WorkerError::Scratch(_) => 2,
            WorkerError::Config(_)
            | WorkerError::OpsListen
            | WorkerError::Nvenc
            | WorkerError::Usage(_)
            | WorkerError::Unset(_)
            | WorkerError::Api(_)
            | WorkerError::Backoff(_) => 3,
        }
    }
}
