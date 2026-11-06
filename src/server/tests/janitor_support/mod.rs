//! Test janitors that inject failures into the real scheduler, a log capture and helpers to read `/metrics`
//! and `/readyz` through the real ops router.

#![allow(dead_code)]

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use contracts::{CycleStats, Fault, Janitor, JanitorError, async_trait};
use sc_clock::{SystemClock, Timestamp};
use sc_observability::{BuildInfo, Component, LineSink, LogConfig, LogFormat, Metrics, subscriber};
use serde_json::Value;
use tower::ServiceExt;
use tracing::Level;
use tracing::subscriber::DefaultGuard;

pub fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn system_clock() -> Arc<SystemClock> {
    Arc::new(SystemClock::new())
}

pub fn metrics() -> Arc<Metrics> {
    Arc::new(
        Metrics::new(BuildInfo {
            version: "0.1.0",
            commit: "test",
            test_clock: false,
        })
        .expect("metrics registry"),
    )
}

#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<Vec<String>>>);

impl Capture {
    /// The captured lines parsed as JSON objects.
    pub fn lines(&self) -> Vec<Value> {
        locked(&self.0)
            .iter()
            .map(|line| serde_json::from_str(line).expect("a JSON log line"))
            .collect()
    }

    /// Error lines whose message names `janitor`.
    pub fn errors_of(&self, janitor: &str) -> Vec<Value> {
        self.lines()
            .into_iter()
            .filter(|line| line["level"] == "error")
            .filter(|line| {
                line["msg"]
                    .as_str()
                    .is_some_and(|msg| msg.contains(janitor))
            })
            .collect()
    }
}

impl LineSink for Capture {
    fn write_line(&self, line: &str) -> io::Result<()> {
        locked(&self.0).push(line.to_owned());
        Ok(())
    }
}

/// Captures the events of the current thread; keep the guard for the test. Use a current-thread runtime
/// (`#[tokio::test]`), so the scheduler's tasks run on the capturing thread.
pub fn capture_logs() -> (Capture, DefaultGuard) {
    let capture = Capture::default();
    let (subscriber, _stats) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        system_clock(),
        Arc::new(capture.clone()),
    );
    (capture, tracing::subscriber::set_default(subscriber))
}

pub async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

pub async fn get(router: &Router, path: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

/// The value of the series of `family` (exported name) whose line carries every `label="value"` pair.
pub fn series(text: &str, family: &str, labels: &[(&str, &str)]) -> Option<f64> {
    text.lines()
        .filter(|line| line.starts_with(family) && line[family.len()..].starts_with('{'))
        .find(|line| {
            labels
                .iter()
                .all(|(name, value)| line.contains(&format!("{name}=\"{value}\"")))
        })
        .and_then(|line| line.rsplit(' ').next())
        .and_then(|value| value.parse().ok())
}

/// Always succeeds; remembers when each cycle started.
pub struct Counting {
    name: &'static str,
    starts: Mutex<Vec<(Timestamp, Instant)>>,
}

impl Counting {
    pub fn new(name: &'static str) -> Arc<Self> {
        Arc::new(Counting {
            name,
            starts: Mutex::new(Vec::new()),
        })
    }

    pub fn calls(&self) -> usize {
        locked(&self.starts).len()
    }

    /// The `now` each cycle was given.
    pub fn nows(&self) -> Vec<Timestamp> {
        locked(&self.starts).iter().map(|(now, _)| *now).collect()
    }

    /// Wall time between consecutive cycle starts.
    pub fn gaps(&self) -> Vec<Duration> {
        let starts = locked(&self.starts);
        starts
            .windows(2)
            .map(|pair| pair[1].1.duration_since(pair[0].1))
            .collect()
    }
}

#[async_trait]
impl Janitor for Counting {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        locked(&self.starts).push((now, Instant::now()));
        Ok(CycleStats::default())
    }
}

/// Fails every cycle.
pub struct Erroring {
    name: &'static str,
    calls: AtomicU64,
}

impl Erroring {
    pub fn new(name: &'static str) -> Arc<Self> {
        Arc::new(Erroring {
            name,
            calls: AtomicU64::new(0),
        })
    }

    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Janitor for Erroring {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run_cycle(&self, _now: Timestamp) -> Result<CycleStats, JanitorError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let fault: Fault = "the owner's store is down".into();
        Err(JanitorError::Unexpected(fault))
    }
}

/// Panics on its first cycle, succeeds afterwards.
pub struct PanicsOnce {
    name: &'static str,
    calls: AtomicU64,
}

impl PanicsOnce {
    pub fn new(name: &'static str) -> Arc<Self> {
        Arc::new(PanicsOnce {
            name,
            calls: AtomicU64::new(0),
        })
    }

    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Janitor for PanicsOnce {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run_cycle(&self, _now: Timestamp) -> Result<CycleStats, JanitorError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("injected panic of the first cycle");
        }
        Ok(CycleStats::default())
    }
}

/// A cycle that takes `take`; notes how many cycles were in progress at once.
pub struct Slow {
    name: &'static str,
    take: Duration,
    started: AtomicU64,
    finished: AtomicU64,
    max_in_flight: AtomicU64,
}

impl Slow {
    pub fn new(name: &'static str, take: Duration) -> Arc<Self> {
        Arc::new(Slow {
            name,
            take,
            started: AtomicU64::new(0),
            finished: AtomicU64::new(0),
            max_in_flight: AtomicU64::new(0),
        })
    }

    pub fn started(&self) -> u64 {
        self.started.load(Ordering::SeqCst)
    }

    pub fn finished(&self) -> u64 {
        self.finished.load(Ordering::SeqCst)
    }

    pub fn max_in_flight(&self) -> u64 {
        self.max_in_flight.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Janitor for Slow {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run_cycle(&self, _now: Timestamp) -> Result<CycleStats, JanitorError> {
        let started = self.started.fetch_add(1, Ordering::SeqCst) + 1;
        let in_flight = started - self.finished.load(Ordering::SeqCst);
        self.max_in_flight.fetch_max(in_flight, Ordering::SeqCst);
        tokio::time::sleep(self.take).await;
        self.finished.fetch_add(1, Ordering::SeqCst);
        Ok(CycleStats::default())
    }
}
