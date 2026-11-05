use std::io;
use std::path::PathBuf;

/// The endpoint cannot be served safely, so the daemon does not serve it. Sync continues;
/// the caller reports the refusal (the server already logged it at ERROR).
#[derive(Debug, thiserror::Error)]
pub enum EndpointRefused {
    #[error("XDG_RUNTIME_DIR is not set to an absolute path")]
    NoRuntimeDir,
    #[error("{path} is not a directory")]
    NotADirectory { path: PathBuf },
    #[error("{path} is not a socket")]
    NotASocket { path: PathBuf },
    #[error("{path} is owned by uid {owner}, not by uid {expected}")]
    ForeignOwner {
        path: PathBuf,
        owner: u32,
        expected: u32,
    },
    #[error("{path} has mode {mode:o}, expected {expected:o}")]
    WrongMode {
        path: PathBuf,
        mode: u32,
        expected: u32,
    },
    #[error("{path} is served by a running process")]
    InUse { path: PathBuf },
    #[error("the pipe {name} already exists")]
    PipeExists { name: String },
    #[error("{action} {target}: {source}")]
    Io {
        action: &'static str,
        target: String,
        source: io::Error,
    },
}

impl EndpointRefused {
    pub(crate) fn io(action: &'static str, target: impl ToString, source: io::Error) -> Self {
        Self::Io {
            action,
            target: target.to_string(),
            source,
        }
    }
}
