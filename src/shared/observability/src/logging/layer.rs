use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use sc_clock::Clock;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use super::event::{AUDIT_TARGET, Counters, Fields, Visitor};
use super::render::{Head, line};
use super::time::rfc3339;
use super::{Component, LogFormat};

/// Where finished lines go. `write_line` gets one line without the newline and writes it whole.
pub trait LineSink: Send + Sync + 'static {
    fn write_line(&self, line: &str) -> io::Result<()>;
}

/// Standard output: one `write_all` per line under the stdout lock, so lines never interleave.
pub struct Stdout;

impl LineSink for Stdout {
    fn write_line(&self, line: &str) -> io::Result<()> {
        use std::io::Write;
        let mut out = io::stdout().lock();
        let mut buffer = Vec::with_capacity(line.len() + 1);
        buffer.extend_from_slice(line.as_bytes());
        buffer.push(b'\n');
        out.write_all(&buffer)
    }
}

pub(crate) struct LogLayer {
    pub(crate) format: LogFormat,
    pub(crate) component: Component,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) sink: Arc<dyn LineSink>,
    pub(crate) counters: Arc<Counters>,
}

/// Request context recorded when a span opens, inherited by the events inside it.
struct SpanFields(Fields);

impl LogLayer {
    fn record(&self, fields: &mut Fields, audit: bool, record: impl FnOnce(&mut Visitor<'_>)) {
        record(&mut Visitor {
            fields,
            audit,
            counters: &self.counters,
        });
    }
}

const fn level_name(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "trace",
    }
}

impl<S> Layer<S> for LogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        self.record(&mut fields, false, |visitor| attrs.record(visitor));
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(fields));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        self.record(&mut fields, false, |visitor| values.record(visitor));
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            match extensions.get_mut::<SpanFields>() {
                Some(existing) => existing.0.overlay(fields),
                None => extensions.insert(SpanFields(fields)),
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let audit = metadata.target() == AUDIT_TARGET;
        let mut fields = Fields::default();
        self.record(&mut fields, audit, |visitor| event.record(visitor));
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(span_fields) = span.extensions().get::<SpanFields>() {
                    fields.inherit(&span_fields.0);
                }
            }
        }
        let ts = rfc3339(self.clock.now_utc());
        let head = Head {
            ts: &ts,
            level: level_name(metadata.level()),
            target: metadata.target(),
            component: self.component.as_str(),
        };
        if self
            .sink
            .write_line(&line(self.format, &head, &fields))
            .is_err()
        {
            self.counters.dropped_lines.fetch_add(1, Ordering::Relaxed);
        }
    }
}
