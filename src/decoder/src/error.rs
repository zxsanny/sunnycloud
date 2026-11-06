use std::io;

use media_shared::decoder_protocol::ProtocolViolation;
use sc_config::ConfigError;
use sc_observability::ObsError;

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Logging(#[from] ObsError),
    #[error("{what}: {source}")]
    Io {
        what: &'static str,
        source: io::Error,
    },
    #[error("the decoder socket path exists and is not a socket")]
    NotASocket,
    #[error("a job did not stop within the cancel wait")]
    JobStuck,
    #[error("a job thread panicked")]
    JobPanicked,
    #[error("unknown subcommand, expected none or `health`")]
    Usage,
    #[error(transparent)]
    Violation(#[from] ProtocolViolation),
}

impl RunnerError {
    pub fn io(what: &'static str) -> impl FnOnce(io::Error) -> RunnerError {
        move |source| RunnerError::Io { what, source }
    }
}
