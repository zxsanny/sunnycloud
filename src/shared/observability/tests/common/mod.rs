#![allow(dead_code)]

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use sc_clock::{ManualClock, Timestamp};
use sc_observability::{Component, LineSink, LogConfig, LogFormat, LogStats, subscriber};
use tracing::Level;

/// 2026-11-06T09:11:12.123456Z
pub const START: Timestamp = Timestamp::from_unix_micros(1_793_956_272_123_456);

#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<Vec<String>>>);

impl Capture {
    pub fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn text(&self) -> String {
        self.lines().join("\n")
    }
}

impl LineSink for Capture {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

pub struct Failing;

impl LineSink for Failing {
    fn write_line(&self, _line: &str) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }
}

pub fn config(format: LogFormat, level: Level) -> LogConfig {
    LogConfig {
        format,
        level,
        component: Component::Api,
    }
}

/// Runs `body` with a capturing subscriber installed on this thread; returns the lines and the counters.
pub fn capture<R>(cfg: LogConfig, body: impl FnOnce() -> R) -> (Capture, LogStats, R) {
    let sink = Capture::default();
    let clock = Arc::new(ManualClock::new(START));
    let (subscriber, stats) = subscriber(cfg, clock, Arc::new(sink.clone()));
    let result = tracing::subscriber::with_default(subscriber, body);
    (sink, stats(), result)
}
