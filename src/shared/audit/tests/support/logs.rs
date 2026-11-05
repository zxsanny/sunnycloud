//! Captures tracing events on the test thread. `sc_observability` renders a line of target `audit` with
//! `"stream":"audit"`; this stand-in does the same, so the tests read the same line shape.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

#[derive(Debug, Clone)]
pub struct Captured {
    pub level: Level,
    pub target: String,
    pub fields: BTreeMap<String, String>,
}

impl Captured {
    /// One JSON object on one line, in the shape of the logging layer.
    pub fn line(&self) -> String {
        let mut line = format!(
            "{{\"level\":\"{}\",\"target\":\"{}\"",
            self.level, self.target
        );
        if self.target == "audit" {
            line.push_str(",\"stream\":\"audit\"");
        }
        for (name, value) in &self.fields {
            let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
            line.push_str(&format!(",\"{name}\":\"{escaped}\""));
        }
        line.push('}');
        line
    }
}

type Store = Arc<Mutex<HashMap<ThreadId, Vec<Captured>>>>;

static STORE: OnceLock<Store> = OnceLock::new();

/// Events of the thread that installed it. One process-wide subscriber serves every test: a per-thread default
/// would race with the callsite interest cache when tests run in parallel, and `#[tokio::test]` runs each test
/// on one thread.
#[derive(Clone)]
pub struct LogCapture {
    thread: ThreadId,
    store: Store,
}

struct Global(Store);

impl LogCapture {
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

    pub fn events(&self) -> Vec<Captured> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&self.thread)
            .cloned()
            .unwrap_or_default()
    }

    pub fn lines(&self) -> Vec<String> {
        self.events().iter().map(Captured::line).collect()
    }

    pub fn audit_lines(&self) -> Vec<Captured> {
        self.events()
            .into_iter()
            .filter(|event| event.target == "audit")
            .collect()
    }

    /// Events that neither belong to the audit stream nor to sqlx's own statement logging.
    pub fn other_lines(&self) -> Vec<Captured> {
        self.events()
            .into_iter()
            .filter(|event| event.target != "audit" && !event.target.starts_with("sqlx"))
            .collect()
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
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
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
                level: *event.metadata().level(),
                target: event.metadata().target().to_owned(),
                fields: fields.0,
            });
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}
