//! AZ-3843 AC-1 (IT-08-16 rotation): the tokens are read from their secret files at load; with
//! `worker_api_token_next` beside the current token both authenticate, and after a restart that dropped the old
//! token it is refused with the same `Forbidden`.

use std::sync::Arc;

use contracts::{WorkerAuthError, WorkerAuthenticator, WorkerIdentity};
use media_api::{JobMetrics, MediaConfig, WorkerAuth};
use sc_observability::{BuildInfo, Metrics, NoLabels};

use crate::support::load_settings_with_tokens;

const OLD: &str = "wk-old-4f1c9a7be2d84c06a3b5e9f1d2c7a8b6";
const NEW: &str = "wk-new-9d2e7b1fa4c64e5b8a0f3c6d1e9b2a74";

fn metrics() -> Arc<JobMetrics> {
    let registry = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    Arc::new(JobMetrics::register(&registry).expect("job metrics"))
}

/// A process start: the settings are loaded from the token files, validated and digested.
fn start(tokens: &[(&str, &str)]) -> (WorkerAuth, Arc<JobMetrics>) {
    let settings = load_settings_with_tokens(tokens).expect("settings");
    let config = MediaConfig::from_settings(&settings);
    assert!(config.validate().is_empty(), "valid configuration");
    let metrics = metrics();
    (WorkerAuth::new(&config, metrics.clone()), metrics)
}

#[tokio::test]
async fn both_tokens_work_while_rotating_and_the_old_one_is_refused_after_the_restart() {
    // Arrange
    let (rotating, _) = start(&[("worker_api_token", OLD), ("worker_api_token_next", NEW)]);
    let (restarted, failures) = start(&[("worker_api_token", NEW)]);

    // Act
    let before = (
        rotating.authenticate(OLD).await,
        rotating.authenticate(NEW).await,
    );
    let after = (
        restarted.authenticate(OLD).await,
        restarted.authenticate(NEW).await,
    );

    // Assert
    assert!(matches!(before, (Ok(WorkerIdentity), Ok(WorkerIdentity))));
    assert!(matches!(
        after,
        (Err(WorkerAuthError::Forbidden), Ok(WorkerIdentity))
    ));
    assert_eq!(failures.worker_auth_failures.get(NoLabels), 1);
}
