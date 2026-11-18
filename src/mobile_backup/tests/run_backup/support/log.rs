//! Captures the events of the current thread as text, so a test can search the log of a run.

use std::fmt::{Debug, Write};
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Metadata, Subscriber};

#[derive(Clone, Default)]
pub struct Capture(Arc<Mutex<String>>);

struct Line<'a>(&'a mut String);

impl Visit for Line<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        write!(self.0, "{}={value:?} ", field.name()).expect("write to a string");
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut text = self.0.lock().expect("log");
        event.record(&mut Line(&mut text));
        text.push('\n');
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

impl Capture {
    /// Events of this thread go here until the guard drops.
    pub fn install() -> (Self, DefaultGuard) {
        let capture = Self::default();
        let guard = tracing::subscriber::set_default(capture.clone());
        (capture, guard)
    }

    pub fn text(&self) -> String {
        self.0.lock().expect("log").clone()
    }
}
