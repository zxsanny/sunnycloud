//! Captures the log events of the test thread (level, target, message and fields) so a test can read the lines
//! the component wrote. One process-wide subscriber serves every test of the binary (a per-thread default races
//! with the callsite interest cache when tests run in parallel); `#[tokio::test]` runs a test on one thread.
//! sqlx's statement events are left out. A binary uses either this or `sc_testdb::statements::StatementLog`, not
//! both.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

#[derive(Debug, Clone)]
pub struct Line {
    pub level: Level,
    pub target: String,
    /// The message and every field, `name=value`, in recorded order.
    pub text: String,
}

type Store = Arc<Mutex<HashMap<ThreadId, Vec<Line>>>>;

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

    pub fn lines(&self) -> Vec<Line> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&self.thread)
            .cloned()
            .unwrap_or_default()
    }

    /// The lines of `level` whose text contains `needle`.
    pub fn at(&self, level: Level, needle: &str) -> Vec<Line> {
        self.lines()
            .into_iter()
            .filter(|line| line.level == level && line.text.contains(needle))
            .collect()
    }
}

struct Text(String);

impl Visit for Text {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if write!(self.0, " {}={value:?}", field.name()).is_err() {
            self.0.push_str(" <unformattable>");
        }
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
        let mut text = Text(String::new());
        event.record(&mut text);
        let line = Line {
            level: *event.metadata().level(),
            target: event.metadata().target().to_owned(),
            text: text.0,
        };
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(std::thread::current().id())
            .or_default()
            .push(line);
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}
