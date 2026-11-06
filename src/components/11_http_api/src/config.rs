use std::net::SocketAddr;
use std::time::Duration;

use crate::error::HttpServerError;

/// The largest flow-control window HTTP/2 allows (RFC 9113 section 6.9.1).
const MAX_H2_WINDOW: u32 = 0x7fff_ffff;

/// The `ServerInfo` value built by 12 (`server_info.md`). Its fields arrive with E-21/c06.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {}

/// The typed section 12 binds through `sc_config` (keys of helper 01; defaults live in its table).
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub public_url: String,
    /// CIDR ranges as written in the key; parsed by the layer that uses them.
    pub trusted_proxies: Vec<String>,
    /// Not in the key table yet (contract open point 1).
    pub public_addr: SocketAddr,
    pub internal_addr: SocketAddr,
    pub ops_addr: SocketAddr,
    pub max_connections: usize,
    pub h1_header_timeout: Duration,
    pub h2_stream_window: u32,
    pub h2_conn_window: u32,
    pub h2_max_streams: u32,
    pub h2_max_pending_reset: usize,
    pub h2_max_header_list: u32,
    pub h2_keepalive_interval: Duration,
    pub h2_keepalive_timeout: Duration,
    pub request_timeout: Duration,
    pub body_idle_timeout: Duration,
    pub shutdown_grace: Duration,
    pub min_client_version: Option<String>,
    pub server_info: ServerInfo,
}

impl HttpConfig {
    /// Refuses a value the accept loop cannot apply; ranges and defaults are `sc_config`'s.
    pub(crate) fn check(&self) -> Result<(), HttpServerError> {
        let invalid = |reason: &str| Err(HttpServerError::InvalidConfig(reason.to_owned()));
        if self.max_connections == 0 {
            return invalid("max_connections must be at least 1");
        }
        if self.h2_max_streams == 0 {
            return invalid("h2_max_streams must be at least 1");
        }
        if self.h2_stream_window > MAX_H2_WINDOW || self.h2_conn_window > MAX_H2_WINDOW {
            return invalid("h2 windows cannot exceed 2147483647 bytes");
        }
        if self.h1_header_timeout.is_zero() {
            return invalid("h1_header_timeout must be above zero");
        }
        if self.h2_keepalive_interval.is_zero() || self.h2_keepalive_timeout.is_zero() {
            return invalid("h2 keepalive interval and timeout must be above zero");
        }
        Ok(())
    }
}
