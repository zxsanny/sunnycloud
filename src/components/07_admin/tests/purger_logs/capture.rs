//! The capturing subscriber of this binary, installed once.

use std::io;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use sc_clock::SystemClock;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use tracing::Level;
use uuid::Uuid;

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

fn lines() -> &'static Lines {
    static LINES: OnceLock<Lines> = OnceLock::new();
    LINES.get_or_init(|| {
        let lines = Lines::default();
        let (capture, _) = subscriber(
            LogConfig {
                format: LogFormat::Json,
                level: Level::INFO,
                component: Component::Api,
            },
            Arc::new(SystemClock::new()),
            Arc::new(lines.clone()),
        );
        tracing::subscriber::set_global_default(capture)
            .expect("no other global subscriber in this binary");
        lines
    })
}

/// Installs the subscriber; call before the code under test runs.
pub fn install() {
    lines();
}

/// The captured lines that carry `account`.
pub fn of_account(account: Uuid) -> Vec<String> {
    containing(&account.hyphenated().to_string())
}

/// The captured lines that contain `needle`, from every test of the binary.
pub fn containing(needle: &str) -> Vec<String> {
    lines()
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|line| line.contains(needle))
        .cloned()
        .collect()
}

pub fn at_level<'a>(lines: &'a [String], level: &str) -> Vec<&'a String> {
    let tag = format!("\"level\":\"{level}\"");
    lines.iter().filter(|line| line.contains(&tag)).collect()
}
