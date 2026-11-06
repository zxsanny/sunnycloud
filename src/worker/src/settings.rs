use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use sc_config::{ConfigSources, WorkerSettings, load};

use crate::error::WorkerError;

/// Loads the worker section from `env` (compiled defaults, `SUNNYCLOUD_*` variables, secret files) and checks
/// the keys the table types as plain text.
pub fn load_settings(env: BTreeMap<String, String>) -> Result<Arc<WorkerSettings>, WorkerError> {
    let sources = ConfigSources {
        env,
        ..ConfigSources::default()
    };
    let settings = load::<WorkerSettings>(&sources)?;
    ops_addr(&settings)?;
    Ok(settings)
}

/// `worker_ops_listen`: `host:port`, or `:port` for every interface.
pub fn ops_addr(settings: &WorkerSettings) -> Result<SocketAddr, WorkerError> {
    parse_listen(&settings.worker_ops_listen).ok_or(WorkerError::OpsListen)
}

fn parse_listen(text: &str) -> Option<SocketAddr> {
    match text.strip_prefix(':') {
        Some(port) => port
            .parse()
            .ok()
            .map(|port| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)),
        None => text.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_port_listens_on_every_interface() {
        assert_eq!(parse_listen(":9090"), Some("0.0.0.0:9090".parse().unwrap()));
    }

    #[test]
    fn host_and_port_parse_as_given() {
        assert_eq!(
            parse_listen("127.0.0.1:1234"),
            Some("127.0.0.1:1234".parse().unwrap())
        );
    }

    #[test]
    fn garbage_is_refused() {
        for text in ["", "9090", ":", ":http", "host:9090", ":70000"] {
            assert_eq!(parse_listen(text), None, "{text}");
        }
    }
}
