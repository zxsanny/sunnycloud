//! Captures the statements sqlx logs on the test thread, so a test can state which statements a call ran, in order
//! with the marks the probe notifier adds ([`mark`]). Same capture as 02's statement-order test: `COMMIT` is kept
//! (from sqlx's `summary`), which 05's own statement log drops.
//! `#[tokio::test]` runs a test on one thread, and one process-wide subscriber serves every test of the binary
//! (a per-thread default races with the callsite interest cache when tests run in parallel).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

type Store = Arc<Mutex<HashMap<ThreadId, Vec<String>>>>;

static STORE: OnceLock<Store> = OnceLock::new();

pub struct StatementLog {
    thread: ThreadId,
    store: Store,
}

struct Global(Store);

impl StatementLog {
    /// Starts an empty log for the current thread.
    pub fn install() -> StatementLog {
        let store = STORE
            .get_or_init(|| {
                let store = Store::default();
                tracing::subscriber::set_global_default(Global(store.clone()))
                    .expect("no other global subscriber in the test binary");
                store
            })
            .clone();
        let log = StatementLog {
            thread: std::thread::current().id(),
            store,
        };
        log.take();
        log
    }

    /// The statements logged since the last call, in order.
    pub fn take(&self) -> Vec<String> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.thread)
            .unwrap_or_default()
    }
}

/// Appends `text` to the current thread's log when a log is installed; a no-op otherwise.
pub fn mark(text: &str) {
    if let Some(store) = STORE.get() {
        store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(std::thread::current().id())
            .or_default()
            .push(text.to_owned());
    }
}

/// sqlx leaves `db.statement` empty for a statement whose `summary` already holds its whole text (`BEGIN`,
/// `SAVEPOINT ...`, `COMMIT`); the summary stands in for it then.
#[derive(Default)]
struct Statement {
    text: Option<String>,
    summary: Option<String>,
}

impl Statement {
    fn sql(self) -> Option<String> {
        match self.text {
            Some(text) if !text.trim().is_empty() => Some(text),
            text => self.summary.or(text),
        }
    }
}

impl Visit for Statement {
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "db.statement" => self.text = Some(value.to_owned()),
            "summary" => self.summary = Some(value.to_owned()),
            _ => {}
        }
    }
}

impl Subscriber for Global {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "sqlx::query"
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut statement = Statement::default();
        event.record(&mut statement);
        if let Some(sql) = statement.sql() {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(std::thread::current().id())
                .or_default()
                .push(sql);
        }
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}
