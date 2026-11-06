//! The typed field API. Each function accepts exactly one type and hands `tracing` a value the formatter
//! accepts; the logging macros call the function named like the field, so a field outside the schema is a
//! compile error.

use std::error::Error;

use sc_error::CorrelationId;
use tracing::Value;
use tracing::field::display;
use uuid::Uuid;

pub fn request_id(id: CorrelationId) -> impl Value {
    display(id)
}

pub fn account_id(id: Uuid) -> impl Value {
    display(id.hyphenated())
}

pub fn node_id(id: Uuid) -> impl Value {
    display(id.hyphenated())
}

/// A route template (`/api/v1/nodes/{id}`), never the request path.
pub fn route(template: &'static str) -> &'static str {
    template
}

pub fn status(status: u16) -> u16 {
    status
}

pub fn duration_ms(milliseconds: u64) -> u64 {
    milliseconds
}

/// The error and its causes.
pub fn err<'a>(error: &'a (dyn Error + 'static)) -> &'a (dyn Error + 'static) {
    error
}
