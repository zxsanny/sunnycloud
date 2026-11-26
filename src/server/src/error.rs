use sc_config::ConfigError;
use sc_error::{DomainError, ErrorCode};
use thiserror::Error;

use crate::command_config::describe;
use crate::exit::Exit;

/// Why a command did not run to the end. Every variant has one exit code.
#[derive(Debug, Error)]
pub enum CliError {
    /// Bad invocation or a refused confirmation: exit 3, nothing on stdout.
    #[error("{0}")]
    Usage(String),
    /// Exit 3, nothing on stdout.
    #[error("{}", describe(.0))]
    Config(#[from] ConfigError),
    /// What an owner port returned: a refusal carries its stable code; an unexpected failure is `internal`.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// An operator-only outcome. Its `code` comes from the command's task until helper 02's table lists it.
    #[error("{detail}")]
    Operator {
        code: &'static str,
        detail: String,
        exit: Exit,
    },
    #[error("command `{0}` is not implemented in this build")]
    NotImplemented(&'static str),
    #[error("service `{0}` is not registered in the context")]
    NotRegistered(&'static str),
    /// A value was provided after its service had been built.
    #[error("service `{0}` was built before the command provided it")]
    AlreadyBuilt(&'static str),
    /// A builder of the composition root failed; the command stops with the builder's own error.
    #[error("cannot build {service}: {source}")]
    Build {
        service: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl CliError {
    pub fn exit(&self) -> Exit {
        match self {
            CliError::Usage(_) | CliError::Config(_) => Exit::Usage,
            CliError::Operator { exit, .. } => *exit,
            CliError::Domain(_)
            | CliError::NotImplemented(_)
            | CliError::NotRegistered(_)
            | CliError::AlreadyBuilt(_)
            | CliError::Build { .. } => Exit::Failed,
        }
    }

    /// Value of `code` in the `--json` envelope.
    pub fn code(&self) -> &'static str {
        match self {
            CliError::Usage(_) | CliError::Config(_) => ErrorCode::InvalidRequest.as_str(),
            CliError::Domain(error) => error.code().as_str(),
            CliError::Operator { code, .. } => code,
            CliError::NotImplemented(_)
            | CliError::NotRegistered(_)
            | CliError::AlreadyBuilt(_)
            | CliError::Build { .. } => ErrorCode::Internal.as_str(),
        }
    }
}
