use std::fmt;
use std::io;
use std::net::SocketAddr;

use sc_observability::ObsError;

use crate::registry::RegistryError;

/// Which listener an error is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listener {
    Public,
    Internal,
    Ops,
    #[cfg(feature = "test-clock")]
    TestClock,
}

impl fmt::Display for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Listener::Public => "public",
            Listener::Internal => "internal",
            Listener::Ops => "ops",
            #[cfg(feature = "test-clock")]
            Listener::TestClock => "test-clock",
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
    /// An operation of the route registry is not fully or consistently declared.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// The connection or HTTP metrics could not be registered (the registry already holds them).
    #[error("http metrics: {0}")]
    Metrics(#[from] ObsError),
}
