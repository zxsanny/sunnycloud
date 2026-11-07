//! AZ-3869 AC-4 (AC-79, ST-09-1): the bearer and `X-Worker-Id` are on every request, and the bearer is in no
//! log line, `Debug` text or error value.

mod api_support;
mod support;

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use api_support::{
    MARKER, Peer, Recorded, Reply, Scratch, WORKER_ID, client, config, file_hash, job, pattern,
};
use media_shared::types::{
    ClaimedJob, DerivedKind, Disposition, FailureReport, JobKind, Lane, OutputMeta, SkipOutcome,
    WorkerStatus,
};
use sc_clock::{ManualClock, Timestamp};
use sc_config::Secret;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sunnycloud_worker::{ApiConfig, ApiConfigError, ApiError, WorkerApiClient, load_settings};
use support::{TokenFile, token_env};
use uuid::Uuid;

const JOB_ID: Uuid = Uuid::from_u128(0x5eed);

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

fn claimed() -> ClaimedJob {
    job(JobKind::Stills, file_hash(&pattern(64)))
}

/// What a healthy API answers on each route, so every call below succeeds.
fn healthy(request: &Recorded) -> Reply {
    let path = request.path.as_str();
    match (request.method.as_str(), path) {
        ("POST", p) if p.ends_with("/claims") => {
            Reply::json(200, &serde_json::to_string(&claimed()).expect("job json"))
        }
        ("PUT", p) if p.ends_with("/lease") => Reply::json(200, r#"{"lease_ms":1000}"#),
        ("GET", _) => Reply::bytes(pattern(64)),
        ("POST", p) if p.ends_with("/failure") => Reply::json(200, r#"{"result":"requeued"}"#),
        _ => Reply::status(204),
    }
}

/// Every call, in order; the value of a success is dropped.
async fn every_call(api: &WorkerApiClient) -> Vec<Result<(), ApiError>> {
    let scratch = Scratch::new(JOB_ID);
    scratch.write_output("webp", b"output bytes");
    let output = scratch.dir.open_output("webp", 1 << 20).expect("output");
    let meta = OutputMeta {
        media_type: "image/webp".into(),
        width: Some(320),
        height: Some(240),
        duration_ms: None,
    };
    vec![
        api.claim(Lane::Image).await.map(drop),
        api.heartbeat(JOB_ID).await.map(drop),
        api.release(JOB_ID).await,
        api.complete(JOB_ID, SkipOutcome::SkippedCompliant).await,
        api.fail(
            JOB_ID,
            &FailureReport {
                code: "corrupt".into(),
                disposition: Disposition::Retry,
            },
        )
        .await
        .map(drop),
        api.report_status(&WorkerStatus {
            decoder_ok: true,
            image_age_days: 3,
        })
        .await,
        api.fetch_input(&claimed(), scratch.source())
            .await
            .map(drop),
        api.post_output(JOB_ID, DerivedKind::Thumb, &meta, output)
            .await,
    ]
}

fn assert_credentials_on_every_request(seen: &[Recorded], expected_count: usize) {
    assert_eq!(seen.len(), expected_count, "one request per call");
    let bearer = format!("Bearer {MARKER}");
    for request in seen {
        assert_eq!(
            request.header("authorization"),
            Some(bearer.as_str()),
            "{}",
            request.path
        );
        assert_eq!(
            request.header("x-worker-id"),
            Some(WORKER_ID),
            "{}",
            request.path
        );
    }
}

#[tokio::test]
async fn every_call_carries_the_bearer_and_the_worker_id() {
    // Arrange
    let peer = Peer::start(healthy).await;
    let api = client(&peer.base_url());

    // Act
    let results = every_call(&api).await;

    // Assert
    for result in &results {
        assert!(result.is_ok(), "{result:?}");
    }
    assert_credentials_on_every_request(&peer.requests(), 8);
}

#[tokio::test]
async fn failing_calls_still_carry_the_credentials_and_leak_the_marker_nowhere() {
    // Arrange: the log subscriber is installed for this thread; the runtime is single-threaded.
    let lines = Lines::default();
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_micros(
        1_793_956_272_123_456,
    )));
    let (log, _stats) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: tracing::Level::TRACE,
            component: Component::Worker,
        },
        clock,
        Arc::new(lines.clone()),
    );
    let _installed = tracing::subscriber::set_default(log);
    let mut texts = Vec::new();
    for status in [403u16, 409, 404, 413, 422, 500, 503, 418] {
        let peer = Peer::start(move |_| Reply::status(status)).await;
        let api = client(&peer.base_url());

        // Act
        let results = every_call(&api).await;

        // Assert
        assert_credentials_on_every_request(&peer.requests(), 8);
        for error in results.into_iter().filter_map(Result::err) {
            texts.push(format!("{error} | {error:?}"));
        }
    }
    let refused = client(&api_support::refused_url().await);
    for error in every_call(&refused)
        .await
        .into_iter()
        .filter_map(Result::err)
    {
        texts.push(format!("{error} | {error:?}"));
    }
    assert!(
        texts.len() > 40,
        "the failing runs produced errors to inspect"
    );
    for text in &texts {
        assert!(!text.contains(MARKER), "bearer in an error: {text}");
    }
    let logged = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .join("\n");
    assert!(!logged.contains(MARKER), "bearer in a log line");
}

#[tokio::test]
async fn the_config_prints_the_bearer_as_stars() {
    // Arrange
    let config = config("http://127.0.0.1:1");

    // Act
    let text = format!("{config:?}");

    // Assert
    assert!(!text.contains(MARKER), "{text}");
    assert!(text.contains("***"), "{text}");
}

#[tokio::test]
async fn an_https_address_is_refused_because_the_worker_listener_is_plain_http() {
    // Arrange
    let secure = config("https://api:8081");

    // Act
    let client = WorkerApiClient::new(secure);

    // Assert
    assert!(matches!(client.err(), Some(ApiConfigError::Url)));
}

#[tokio::test]
async fn a_bearer_or_worker_id_that_is_no_header_value_is_refused_without_echoing_it() {
    // Arrange
    let mut bad_token = config("http://127.0.0.1:1");
    bad_token.token = Secret::new(format!("{MARKER}\nX-Injected: 1"));
    let mut bad_id = config("http://127.0.0.1:1");
    bad_id.worker_id = "id\r\n".into();
    let mut bad_url = config("ftp://api:8081");
    bad_url.base_url = "ftp://api:8081".into();

    // Act
    let token = WorkerApiClient::new(bad_token);
    let id = WorkerApiClient::new(bad_id);
    let url = WorkerApiClient::new(bad_url);

    // Assert
    let token = token.err().expect("token refused");
    assert!(matches!(token, ApiConfigError::Token), "{token:?}");
    assert!(!format!("{token} {token:?}").contains(MARKER));
    assert!(matches!(id.err(), Some(ApiConfigError::WorkerId)));
    assert!(matches!(url.err(), Some(ApiConfigError::Url)));
}

#[tokio::test]
async fn a_redirect_is_not_followed_so_the_bearer_goes_nowhere_else() {
    // Arrange
    let elsewhere = Peer::start(|_| Reply::status(204)).await;
    let target = format!("{}/steal", elsewhere.base_url());
    let redirecting = Peer::start(move |_| Reply {
        status: 307,
        headers: vec![("location", target.clone())],
        body: Vec::new(),
    })
    .await;

    // Act
    let result = client(&redirecting.base_url()).release(JOB_ID).await;

    // Assert
    assert!(matches!(result, Err(ApiError::Protocol(_))), "{result:?}");
    assert!(elsewhere.requests().is_empty());
}

#[tokio::test]
async fn a_client_built_from_the_worker_settings_sends_the_secret_file_token() {
    // Arrange
    let peer = Peer::start(|_| Reply::status(204)).await;
    let token = TokenFile::new(MARKER);
    let mut env = token_env(&token);
    env.insert("SUNNYCLOUD_WORKER_API_URL".into(), peer.base_url());
    let settings = load_settings(env).expect("settings load");
    let api = WorkerApiClient::new(ApiConfig::from_settings(&settings, WORKER_ID.into()))
        .expect("client");

    // Act
    let result = api.release(JOB_ID).await;

    // Assert
    result.expect("released");
    assert_credentials_on_every_request(&peer.requests(), 1);
}
