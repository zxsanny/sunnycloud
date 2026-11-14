//! `WorkerAuthenticator` (E-18/c08): the worker supervisor's bearer secret, and nothing a user holds. Only the
//! SHA-256 digests of `worker_api_token` and, while it is rotated, `worker_api_token_next` are kept; the presented
//! secret is digested and compared in constant time with every kept digest. Every refusal is the one `Forbidden`.

use std::sync::Arc;

use contracts::{WorkerAuthError, WorkerAuthenticator, WorkerIdentity, async_trait};
use sc_ids::{TokenHash, sha256_token};
use sc_observability::{NoLabels, log_warn};

use crate::config::MediaConfig;
use crate::job_metrics::JobMetrics;

/// No `Debug`: it would print the digests.
pub struct WorkerAuth {
    digests: Vec<TokenHash>,
    metrics: Arc<JobMetrics>,
}

impl WorkerAuth {
    /// Digests the configured tokens once; the configuration is expected to be validated (no empty token).
    pub fn new(config: &MediaConfig, metrics: Arc<JobMetrics>) -> Self {
        let digests = std::iter::once(&config.worker_api_token)
            .chain(config.worker_api_token_next.as_ref())
            .map(|token| sha256_token(token.expose()))
            .collect();
        WorkerAuth { digests, metrics }
    }

    /// Every kept digest is compared, so the time does not tell which one matched. An empty bearer never matches,
    /// even under a configuration that skipped validation.
    fn accepts(&self, bearer: &str) -> bool {
        let presented = sha256_token(bearer);
        let matched = self
            .digests
            .iter()
            .fold(false, |matched, digest| matched | (*digest == presented));
        matched && !bearer.is_empty()
    }
}

#[async_trait]
impl WorkerAuthenticator for WorkerAuth {
    /// `bearer` is the token after `Bearer `; a missing header arrives as an empty string.
    async fn authenticate(&self, bearer: &str) -> Result<WorkerIdentity, WorkerAuthError> {
        if self.accepts(bearer) {
            return Ok(WorkerIdentity);
        }
        self.metrics.worker_auth_failures.inc(NoLabels);
        log_warn!("worker authentication refused");
        Err(WorkerAuthError::Forbidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_config::Secret;
    use sc_observability::{BuildInfo, Metrics};
    use std::time::Duration;

    const CURRENT: &str = "wk-current-0123456789abcdef0123456789abcdef";
    const NEXT: &str = "wk-next-fedcba9876543210fedcba9876543210";

    fn config(next: Option<&str>) -> MediaConfig {
        let second = Duration::from_secs(1);
        MediaConfig {
            job_lease: second,
            job_attempts: 3,
            job_retry_backoff: second,
            bulk_queue_threshold: 5,
            job_terminal_ttl: second,
            job_reap_interval: second,
            derived_janitor_interval: second,
            worker_status_interval: second,
            derived_max_thumb: 1,
            derived_max_preview: 1,
            derived_max_transcode: 1,
            worker_api_token: Secret::new(CURRENT.to_owned()),
            worker_api_token_next: next.map(|token| Secret::new(token.to_owned())),
        }
    }

    fn auth(next: Option<&str>) -> WorkerAuth {
        let metrics = Metrics::new(BuildInfo {
            version: "test",
            commit: "test",
            test_clock: true,
        })
        .expect("metrics");
        WorkerAuth::new(
            &config(next),
            Arc::new(JobMetrics::register(&metrics).expect("job metrics")),
        )
    }

    fn with_byte(text: &str, index: usize) -> String {
        let mut bytes = text.as_bytes().to_vec();
        bytes[index] ^= 0x01;
        String::from_utf8(bytes).expect("ascii stays utf-8")
    }

    #[tokio::test]
    async fn secrets_differing_in_the_first_or_the_last_byte_get_the_same_refusal() {
        // Arrange
        let auth = auth(None);
        let first = with_byte(CURRENT, 0);
        let last = with_byte(CURRENT, CURRENT.len() - 1);

        // Act
        let results = [
            auth.authenticate(&first).await,
            auth.authenticate(&last).await,
            auth.authenticate(CURRENT).await,
        ];

        // Assert
        assert!(matches!(results[0], Err(WorkerAuthError::Forbidden)));
        assert!(matches!(results[1], Err(WorkerAuthError::Forbidden)));
        assert!(matches!(results[2], Ok(WorkerIdentity)));
        assert_eq!(auth.metrics.worker_auth_failures.get(NoLabels), 2);
    }

    #[test]
    fn only_digests_are_kept_one_per_configured_token() {
        // Arrange
        let single = auth(None);
        let rotating = auth(Some(NEXT));

        // Act
        let kept = (single.digests.clone(), rotating.digests.clone());

        // Assert
        assert_eq!(kept.0, vec![sha256_token(CURRENT)]);
        assert_eq!(kept.1, vec![sha256_token(CURRENT), sha256_token(NEXT)]);
    }

    #[test]
    fn the_digest_set_with_the_next_token_accepts_both_and_without_it_only_the_current() {
        // Arrange
        let single = auth(None);
        let rotating = auth(Some(NEXT));

        // Act
        let accepted = [
            single.accepts(CURRENT),
            single.accepts(NEXT),
            rotating.accepts(CURRENT),
            rotating.accepts(NEXT),
        ];

        // Assert
        assert_eq!(accepted, [true, false, true, true]);
    }

    #[test]
    fn an_empty_bearer_is_refused_even_when_an_empty_token_was_configured() {
        // Arrange
        let mut empty = config(None);
        empty.worker_api_token = Secret::new(String::new());
        let metrics = Metrics::new(BuildInfo {
            version: "test",
            commit: "test",
            test_clock: true,
        })
        .expect("metrics");
        let auth = WorkerAuth::new(
            &empty,
            Arc::new(JobMetrics::register(&metrics).expect("job metrics")),
        );

        // Act
        let accepted = auth.accepts("");

        // Assert
        assert!(!accepted);
    }
}
