use sc_error::ErrorCode;
use serde_json::{Value, json};

/// The `Problem` schema for the OpenAPI components (`problem_details.md`), listing every code of the table.
pub fn problem_schema() -> Value {
    let codes: Vec<&str> = ErrorCode::ALL.iter().map(|code| code.as_str()).collect();
    json!({
        "type": "object",
        "required": ["type", "title", "status", "code", "request_id"],
        "properties": {
            "type": {"type": "string", "pattern": "^urn:sunnycloud:error:[a-z_]+$"},
            "title": {"type": "string"},
            "status": {"type": "integer"},
            "code": {"type": "string", "enum": codes},
            "request_id": {"type": "string", "format": "uuid"},
            "retry_after": {"type": "integer", "minimum": 0},
            "need_blocks": {"type": "array", "items": {"type": "integer", "minimum": 0}},
            "reason": {
                "type": "string",
                "enum": ["epoch_changed", "below_purge_horizon", "cursor_ahead"]
            },
            "min_version": {"type": "string"},
            "update_url": {"type": "string"},
            "problems": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["field", "problem"],
                    "properties": {
                        "field": {"type": "string"},
                        "problem": {"type": "string"}
                    }
                }
            }
        }
    })
}
