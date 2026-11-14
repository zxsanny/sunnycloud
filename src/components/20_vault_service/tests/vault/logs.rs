//! Captures the log events of the test thread (target and fields) so a test can read the audit lines the
//! service wrote. One process-wide subscriber serves every test of the binary (a per-thread default races with
//! the callsite interest cache when tests run in parallel); `#[tokio::test]` runs a test on one thread.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// One event: its target and every recorded field (the message is the field `message`).
#[derive(Debug, Clone)]
pub struct Captured {
    pub target: String,
    pub fields: BTreeMap<String, String>,
}

type Store = Arc<Mutex<HashMap<ThreadId, Vec<Captured>>>>;

static STORE: OnceLock<Store> = OnceLock::new();

pub struct LogCapture {
    thread: ThreadId,
    store: Store,
}

struct Global(Store);

impl LogCapture {
    /// Starts an empty capture for the current thread.
    pub fn install() -> LogCapture {
        let store = STORE
            .get_or_init(|| {
                let store = Store::default();
                tracing::subscriber::set_global_default(Global(store.clone()))
                    .expect("no other global subscriber in the test binary");
                store
            })
            .clone();
        let thread = std::thread::current().id();
        store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&thread);
        LogCapture { thread, store }
    }

    /// The lines of target `audit` (`"stream":"audit"` once rendered by the logging layer).
    pub fn audit_lines(&self) -> Vec<Captured> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&self.thread)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event.target == "audit")
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

impl Subscriber for Global {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        !metadata.target().starts_with("sqlx")
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields(BTreeMap::new());
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(std::thread::current().id())
            .or_default()
            .push(Captured {
                target: event.metadata().target().to_owned(),
                fields: fields.0,
            });
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}
