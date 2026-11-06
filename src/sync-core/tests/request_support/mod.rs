#![allow(dead_code)]
//! Shared pieces of the request-path tests: a transport that talks to the scripted fake server,
//! a credential provider with sentinel tokens, and a capture of `tracing` events.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use sc_clock::{Clock, ManualClock, MonotonicInstant, Timestamp};
use sc_config::Secret;
use sc_error::TransportKind;
use sync_core::{
    BoxFuture, Credential, CredentialProvider, Method, PersistFailed, Request, Transport,
    WireRequest, WireResponse,
};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, registry};

use crate::support::{Client, Fixture};

pub const ACCESS: &str = "ACC-SENTINEL";
pub const REFRESH: &str = "REF-SENTINEL";

pub struct SentinelCredentials {
    pub bearer_calls: AtomicU32,
}

impl SentinelCredentials {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            bearer_calls: AtomicU32::new(0),
        })
    }
}

impl CredentialProvider for SentinelCredentials {
    fn bearer(&self) -> Credential {
        self.bearer_calls.fetch_add(1, Ordering::SeqCst);
        Credential {
            access: Secret::new(ACCESS.to_owned()),
            refresh: Some(Secret::new(REFRESH.to_owned())),
        }
    }

    fn on_rotated(
        &self,
        _access: Secret<String>,
        _refresh: Secret<String>,
    ) -> Result<(), PersistFailed> {
        Ok(())
    }
}

/// One TLS connection per request to the fixture; the access token goes into `Authorization` only.
pub struct FixtureTransport {
    fixture_addr: std::net::SocketAddr,
    config: Arc<rustls::ClientConfig>,
    clock: Arc<ManualClock>,
    pub sent_at: Mutex<Vec<MonotonicInstant>>,
}

impl FixtureTransport {
    pub fn new(fixture: &Fixture, clock: Arc<ManualClock>) -> Arc<Self> {
        Arc::new(Self {
            fixture_addr: fixture.addr(),
            config: fixture.client_config(&[&rustls::version::TLS13], &[b"h2"]),
            clock,
            sent_at: Mutex::new(Vec::new()),
        })
    }

    pub fn sent_at(&self) -> Vec<MonotonicInstant> {
        self.sent_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Transport for FixtureTransport {
    fn send(&self, request: WireRequest) -> BoxFuture<'_, Result<WireResponse, TransportKind>> {
        Box::pin(async move {
            self.sent_at
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(self.clock.monotonic());
            let mut headers: Vec<(String, String)> = request.headers.clone();
            if let Some(bearer) = &request.bearer {
                headers.push((
                    "authorization".to_owned(),
                    format!("Bearer {}", bearer.expose()),
                ));
            }
            let pairs: Vec<(&str, &str)> = headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let mut client = Client::connect(self.fixture_addr, "127.0.0.1", self.config.clone())
                .await
                .map_err(|_| TransportKind::Connect)?;
            let reply = client
                .send(
                    request.method.as_str(),
                    &request.target,
                    &pairs,
                    request.body,
                )
                .await
                .map_err(|_| TransportKind::Other)?;
            Ok(WireResponse {
                status: reply.status,
                headers: reply.headers,
                body: reply.body,
            })
        })
    }
}

pub fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Timestamp::from_unix_micros(
        1_793_956_272_000_000,
    )))
}

pub fn get(target: &str, max_attempts: u32) -> Request {
    Request {
        method: Method::Get,
        target: target.to_owned(),
        headers: Vec::new(),
        body: Bytes::new(),
        max_attempts,
    }
}

/// Waits (real time, short steps) until `check` holds; panics after `limit_ms`.
pub async fn eventually(limit_ms: u64, what: &str, mut check: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(limit_ms);
    while !check() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[derive(Clone, Default)]
pub struct LogCapture(Arc<Mutex<Vec<String>>>);

impl LogCapture {
    pub fn text(&self) -> String {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .join("\n")
    }

    pub fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

struct Collect<'a>(&'a mut String);

impl Visit for Collect<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // Writing to a String cannot fail.
        write!(self.0, " {}={:?}", field.name(), value).expect("write to String");
    }
}

impl<S: Subscriber> Layer<S> for LogCapture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut line = format!("{} {}", event.metadata().level(), event.metadata().target());
        event.record(&mut Collect(&mut line));
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }
}

/// Captures every event, DEBUG and up, on this thread until the guard drops. Use with a current-thread runtime.
pub fn capture_logs() -> (LogCapture, tracing::subscriber::DefaultGuard) {
    let capture = LogCapture::default();
    let guard = tracing::subscriber::set_default(registry().with(capture.clone()));
    (capture, guard)
}

/// Ticks `clock` until the task ends, so retry waits pass without real time; fails after 10 minutes of clock time.
pub async fn drive<T>(clock: &ManualClock, task: tokio::task::JoinHandle<T>) -> T {
    let mut moved = std::time::Duration::ZERO;
    while !task.is_finished() {
        assert!(
            moved < std::time::Duration::from_secs(600),
            "task never finished"
        );
        clock.tick(std::time::Duration::from_millis(250));
        moved += std::time::Duration::from_millis(250);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    task.await.expect("request task")
}
