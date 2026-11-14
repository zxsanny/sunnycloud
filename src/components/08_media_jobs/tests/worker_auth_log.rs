//! AZ-3843 AC-79 (IT-08-16 port part, NFT-SEC-50): no header, an empty bearer, a wrong secret, a user session
//! token, a device token and a session cookie each get the one `Forbidden`; the worker secret gets the worker
//! identity; the failure counter rises by 6 and no log line holds a presented secret. The user credentials are
//! real tokens of their kinds (`sc_ids::new_token`, the generator 01 uses). This binary holds this one test: the
//! capturing subscriber is installed on the test's thread, and a scoped default races with the callsite
//! interest cache when other tests run in parallel.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{WorkerAuthError, WorkerAuthenticator, WorkerIdentity};
use media_api::{JobMetrics, MediaConfig, WorkerAuth};
use sc_clock::ManualClock;
use sc_config::Secret;
use sc_ids::{TokenKind, new_token};
use sc_observability::{
    BuildInfo, Component, LineSink, LogConfig, LogFormat, Metrics, NoLabels, subscriber,
};
use tracing::Level;

const WORKER_SECRET: &str = "wk-3b8f0c2d6e1a4f97b5c8d0e2f4a6b8c1d3e5f7a9";

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

fn config() -> MediaConfig {
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
        worker_api_token: Secret::new(WORKER_SECRET.to_owned()),
        worker_api_token_next: None,
    }
}

fn token(kind: TokenKind) -> String {
    new_token(kind).expect("rng").expose().to_owned()
}

#[tokio::test]
async fn every_non_worker_credential_gets_the_one_forbidden_counted_and_never_logged() {
    // Arrange
    let metrics = Arc::new(
        JobMetrics::register(
            &Metrics::new(BuildInfo {
                version: "test",
                commit: "test",
                test_clock: true,
            })
            .expect("metrics"),
        )
        .expect("job metrics"),
    );
    let auth = WorkerAuth::new(&config(), metrics.clone());
    let session = token(TokenKind::Session);
    let presented = [
        // A missing `Authorization` header reaches the port as an empty bearer.
        String::new(),
        " ".to_owned(),
        "wk-3b8f0c2d6e1a4f97b5c8d0e2f4a6b8c1d3e5f7a8".to_owned(),
        session.clone(),
        token(TokenKind::DeviceToken),
        format!("__Host-session={session}"),
    ];
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::DEBUG,
            component: Component::Api,
        },
        Arc::new(ManualClock::new(sc_testdb::START)),
        Arc::new(lines.clone()),
    );
    let _guard = tracing::subscriber::set_default(capture);

    // Act
    let mut refusals = Vec::new();
    for bearer in &presented {
        refusals.push(auth.authenticate(bearer).await);
    }
    let worker = auth.authenticate(WORKER_SECRET).await;

    // Assert
    for (bearer, refusal) in presented.iter().zip(&refusals) {
        let Err(error) = refusal else {
            panic!("accepted {:?}", bearer.get(..7));
        };
        assert!(matches!(error, WorkerAuthError::Forbidden));
        assert_eq!(
            (format!("{error:?}"), error.to_string()),
            ("Forbidden".to_owned(), "forbidden".to_owned())
        );
    }
    assert!(matches!(worker, Ok(WorkerIdentity)));
    assert_eq!(metrics.worker_auth_failures.get(NoLabels), 6);
    let logged = lines.all();
    let warns = logged
        .iter()
        .filter(|line| {
            line.contains("\"level\":\"warn\"") && line.contains("worker authentication refused")
        })
        .count();
    assert_eq!(warns, 6, "{logged:?}");
    for line in &logged {
        for secret in presented
            .iter()
            .map(String::as_str)
            .chain([WORKER_SECRET, session.as_str()])
            .filter(|secret| secret.trim().len() > 7)
        {
            assert!(!line.contains(secret), "a secret was logged");
        }
    }
}
