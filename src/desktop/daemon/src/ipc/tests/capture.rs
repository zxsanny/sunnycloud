use std::fmt::Write;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Level, Metadata, Subscriber};

/// Records every tracing event as `(level, "field=value ...")`.
#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<(Level, String)>>>);

impl Captured {
    /// Installs the collector for the current thread until the guard drops.
    pub fn install(&self) -> DefaultGuard {
        tracing::subscriber::set_default(Collector(self.clone()))
    }

    pub fn events(&self) -> Vec<(Level, String)> {
        self.0.lock().unwrap().clone()
    }
}

struct Collector(Captured);

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        write!(self.0, "{}={value:?} ", field.name()).unwrap();
    }
}

impl Subscriber for Collector {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        (self.0.0.lock().unwrap()).push((*event.metadata().level(), fields.0));
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}
