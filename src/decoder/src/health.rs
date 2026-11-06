//! `decoder-runner health`: informational (10 section 8). Restarts come from the self-abort,
//! not from the container healthcheck, and the socket serves one connection at a time, so this
//! never connects: it checks that the config binds and that the socket file exists.

use sc_config::ConfigSources;

use crate::config::RunnerConfig;
use crate::socket::is_socket;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unhealthy {
    ConfigInvalid,
    SocketMissing,
}

impl Unhealthy {
    pub fn reason(self) -> &'static str {
        match self {
            Unhealthy::ConfigInvalid => "config_invalid",
            Unhealthy::SocketMissing => "socket_missing",
        }
    }
}

pub fn check(sources: &ConfigSources) -> Result<(), Unhealthy> {
    let config = RunnerConfig::load(sources).map_err(|_| Unhealthy::ConfigInvalid)?;
    if is_socket(&config.socket_path) {
        Ok(())
    } else {
        Err(Unhealthy::SocketMissing)
    }
}
