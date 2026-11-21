//! The JSON of the session routes.

use contracts::SessionInfo;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::registry::SchemaRef;
use crate::timestamp::rfc3339;

/// `{items: [{id, label, client_kind, ip, current, created_at, last_seen_at}]}`; `ip` is `null` for a session
/// signed in before the address was recorded, `current` marks the caller's own session.
pub(super) fn session_list(sessions: &[SessionInfo], current: Uuid) -> Value {
    let items: Vec<Value> = sessions
        .iter()
        .map(|session| {
            json!({
                "id": session.id,
                "label": session.label,
                "client_kind": session.client_kind,
                "ip": session.ip.map(|ip| ip.to_string()),
                "current": session.id == current,
                "created_at": rfc3339(session.created_at),
                "last_seen_at": rfc3339(session.last_seen_at),
            })
        })
        .collect();
    json!({"items": items})
}

pub(super) const SESSION_LIST: SchemaRef = SchemaRef {
    name: "SessionList",
    build: || {
        json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "label", "client_kind", "ip", "current", "created_at", "last_seen_at"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"},
                            "label": {"type": "string"},
                            "client_kind": {"type": "string", "enum": ["web", "android", "ios"]},
                            "ip": {"type": ["string", "null"]},
                            "current": {"type": "boolean"},
                            "created_at": {"type": "string"},
                            "last_seen_at": {"type": "string"}
                        }
                    }
                }
            }
        })
    },
};
