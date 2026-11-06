//! Captures the statements sqlx logs on the test thread, so a test can state which statements a call ran.
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

    /// Like [`StatementLog::take`], trimmed, without the scope and transaction statements of `sc_db`
    /// (sqlx logs `BEGIN` and `COMMIT` with an empty text).
    pub fn take_data_statements(&self) -> Vec<String> {
        self.take()
            .into_iter()
            .map(|sql| sql.trim().to_owned())
            .filter(|sql| !sql.is_empty() && !sql.contains("set_config("))
            .collect()
    }
}

struct Statement(Option<String>);

impl Visit for Statement {
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "db.statement" {
            self.0 = Some(value.to_owned());
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
        let mut statement = Statement(None);
        event.record(&mut statement);
        if let Some(sql) = statement.0 {
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
