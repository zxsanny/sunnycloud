//! What an event may carry. Anything outside the field schema is dropped and counted, never rendered; a value
//! of the wrong shape (an id that is not a UUID, a status that is not an HTTP status) is dropped the same way.

use std::error::Error;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use tracing::field::{Field, Visit};
use uuid::Uuid;

/// Target whose lines carry `"stream":"audit"` and the fields of an audit event.
pub const AUDIT_TARGET: &str = "audit";

/// Fields `sc_audit` mirrors into its log line (ids, enumerations, an address and an id/code/integer detail).
const AUDIT_FIELDS: &[&str] = &[
    "event",
    "actor_account_id",
    "actor_session_id",
    "subject_type",
    "subject_id",
    "ip",
    "detail",
];

const MAX_ROUTE_LEN: usize = 128;
const MAX_SEGMENT_LEN: usize = 32;
const MAX_ERR_LEN: usize = 512;

/// Counts of what the logging layer could not do, read through `LogGuard::stats`.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) dropped_lines: AtomicU64,
    pub(crate) rejected_fields: AtomicU64,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Fields {
    pub(crate) msg: String,
    pub(crate) request_id: Option<String>,
    pub(crate) account_id: Option<String>,
    pub(crate) node_id: Option<String>,
    pub(crate) route: Option<String>,
    pub(crate) status: Option<u16>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) err: Option<String>,
    pub(crate) audit: Vec<(&'static str, Value)>,
}

impl Fields {
    /// Takes the request context of an enclosing span for what the event itself did not set.
    pub(crate) fn inherit(&mut self, span: &Fields) {
        fill(&mut self.request_id, &span.request_id);
        fill(&mut self.account_id, &span.account_id);
        fill(&mut self.node_id, &span.node_id);
        fill(&mut self.route, &span.route);
    }

    pub(crate) fn overlay(&mut self, later: Fields) {
        for (slot, value) in [
            (&mut self.request_id, later.request_id),
            (&mut self.account_id, later.account_id),
            (&mut self.node_id, later.node_id),
            (&mut self.route, later.route),
        ] {
            if value.is_some() {
                *slot = value;
            }
        }
    }
}

fn fill(slot: &mut Option<String>, from: &Option<String>) {
    if slot.is_none() {
        slot.clone_from(from);
    }
}

pub(crate) struct Visitor<'a> {
    pub(crate) fields: &'a mut Fields,
    pub(crate) audit: bool,
    pub(crate) counters: &'a Counters,
}

impl Visitor<'_> {
    fn reject(&self) {
        self.counters
            .rejected_fields
            .fetch_add(1, Ordering::Relaxed);
    }

    fn text(&mut self, name: &'static str, text: &str) {
        let fields = &mut *self.fields;
        let accepted = match name {
            "message" => {
                fields.msg = text.to_owned();
                true
            }
            "request_id" => uuid_text(text)
                .map(|id| fields.request_id = Some(id))
                .is_some(),
            "account_id" => uuid_text(text)
                .map(|id| fields.account_id = Some(id))
                .is_some(),
            "node_id" => uuid_text(text)
                .map(|id| fields.node_id = Some(id))
                .is_some(),
            "route" if is_route(text) => {
                fields.route = Some(text.to_owned());
                true
            }
            "err" => {
                fields.err = Some(text.chars().take(MAX_ERR_LEN).collect());
                true
            }
            _ if self.audit && AUDIT_FIELDS.contains(&name) => {
                fields.audit.push((name, Value::String(text.to_owned())));
                true
            }
            _ => false,
        };
        if !accepted {
            self.reject();
        }
    }

    fn number(&mut self, name: &'static str, value: u64) {
        let fields = &mut *self.fields;
        let accepted = match name {
            "status" => u16::try_from(value)
                .ok()
                .filter(|status| (100..=599).contains(status))
                .map(|status| fields.status = Some(status))
                .is_some(),
            "duration_ms" => {
                fields.duration_ms = Some(value);
                true
            }
            _ if self.audit && AUDIT_FIELDS.contains(&name) => {
                fields.audit.push((name, Value::from(value)));
                true
            }
            _ => false,
        };
        if !accepted {
            self.reject();
        }
    }
}

impl Visit for Visitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.text(field.name(), value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.number(field.name(), value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        match u64::try_from(value) {
            Ok(value) => self.number(field.name(), value),
            Err(_) => self.reject(),
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        self.text(field.name(), &error_chain(value));
    }

    // `display(..)` wrappers and `format_args!` arrive here; their Debug is their Display text.
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.text(field.name(), &format!("{value:?}"));
    }
}

pub(crate) fn error_chain(error: &(dyn Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn uuid_text(text: &str) -> Option<String> {
    if text.len() != 36 {
        return None;
    }
    Uuid::parse_str(text).ok().map(|id| id.to_string())
}

/// A route template as the router registers it: `/` then lower case segments (`[a-z0-9_-]`, at most 32
/// characters) or `{param}` placeholders. A token, an address, a file name or an e-mail address is not one.
fn is_route(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('/') else {
        return false;
    };
    text.len() <= MAX_ROUTE_LEN
        && (rest.is_empty()
            || rest.split('/').all(|segment| {
                let literal = |word: &str| {
                    !word.is_empty()
                        && word.len() <= MAX_SEGMENT_LEN
                        && word.chars().all(|c| {
                            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'
                        })
                };
                match segment
                    .strip_prefix('{')
                    .and_then(|inner| inner.strip_suffix('}'))
                {
                    Some(param) => literal(param),
                    None => literal(segment),
                }
            }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_templates_pass_and_everything_else_fails() {
        for ok in [
            "/",
            "/healthz",
            "/api/v1/nodes/{id}",
            "/api/v1/share-links/{token_id}/open",
        ] {
            assert!(is_route(ok), "{ok}");
        }
        for bad in [
            "",
            "api/v1",
            "/home/alice/Tax Return.pdf",
            "/api/v1/nodes/Alice",
            "/a.b",
            "/api//x",
            "/{}",
            "alice@mail.test",
            "/0123456789012345678901234567890123456789",
        ] {
            assert!(!is_route(bad), "{bad}");
        }
    }

    #[test]
    fn ids_must_be_hyphenated_uuids_and_come_back_canonical() {
        assert_eq!(
            uuid_text("0198D5A1-1111-7222-8333-444455556666").as_deref(),
            Some("0198d5a1-1111-7222-8333-444455556666")
        );
        assert_eq!(uuid_text("0198d5a1111172228333444455556666"), None);
        assert_eq!(uuid_text("{0198d5a1-1111-7222-8333-444455556666}"), None);
        assert_eq!(uuid_text("at_7f3a9c1e2b4d6f80a1b2c3d4e5f60718"), None);
    }

    #[test]
    fn an_error_chain_lists_the_causes() {
        #[derive(Debug, thiserror::Error)]
        #[error("outer")]
        struct Outer(#[source] std::io::Error);
        let error = Outer(std::io::Error::other("inner"));
        assert_eq!(error_chain(&error), "outer: inner");
    }
}
