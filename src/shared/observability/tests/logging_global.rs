//! `init_logging` installs the process-wide subscriber once. Own test binary: the global subscriber is
//! per process.

use sc_observability::{Component, LogConfig, LogFormat, ObsError, init_logging, log_info};
use tracing::Level;

#[test]
fn init_logging_installs_once_and_a_second_call_is_already_installed() {
    let cfg = LogConfig {
        format: LogFormat::Json,
        level: Level::INFO,
        component: Component::Worker,
    };

    let guard = init_logging(cfg).expect("first install");
    log_info!("installed");
    let second = init_logging(cfg);

    assert!(
        matches!(second, Err(ObsError::AlreadyInstalled)),
        "{:?}",
        second.err()
    );
    assert_eq!(guard.stats().dropped_lines, 0);
    assert_eq!(guard.stats().rejected_fields, 0);
}
