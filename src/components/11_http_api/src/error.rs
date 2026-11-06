use std::fmt;
use std::io;
use std::net::SocketAddr;

use sc_observability::ObsError;

/// Which listener an error is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listener {
    Public,
    Internal,
}

impl fmt::Display for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Listener::Public => "public",
            Listener::Internal => "internal",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpServerError {
    /// A configuration value the server cannot use.
    #[error("invalid http configuration: {0}")]
    InvalidConfig(String),
    /// Raised before any listener serves.
    #[error("cannot bind the {listener} listener on {addr}")]
    Bind {
        listener: Listener,
        addr: SocketAddr,
        #[source]
        source: io::Error,
    },
    /// The connection metrics could not be registered (the registry already holds them).
    #[error("connection metrics: {0}")]
    Metrics(#[from] ObsError),
}
