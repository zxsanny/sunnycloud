//! The JSON of the session routes.

use contracts::SessionInfo;
use serde_json::{Value, json};

use crate::registry::SchemaRef;
use crate::timestamp::rfc3339;

/// `{items: [{id, label, created_at, last_seen_at}]}`.
pub(super) fn session_list(sessions: &[SessionInfo]) -> Value {
    let items: Vec<Value> = sessions
        .iter()
        .map(|session| {
            json!({
                "id": session.id,
                "label": session.label,
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
                        "required": ["id", "label", "created_at", "last_seen_at"],
                        "properties": {
                            "id": {"type": "string", "format": "uuid"},
                            "label": {"type": "string"},
                            "created_at": {"type": "string"},
                            "last_seen_at": {"type": "string"}
                        }
                    }
                }
            }
        })
    },
};
