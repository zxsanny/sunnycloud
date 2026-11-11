//! The JSON of the change feed. The names are those of the DTOs the clients already hold (`sync_core_proto`);
//! the cursor is the text `E.S`.

use contracts::{ChangeItem, ChangePage, Cursor};
use serde_json::{Value, json};

use crate::registry::SchemaRef;
use crate::registry::tree::wire::{kind, state};

/// `E.S`: two unsigned decimal numbers joined by a dot.
pub(super) fn parse_cursor(text: &str) -> Option<Cursor> {
    let (epoch, seq) = text.split_once('.')?;
    Some(Cursor {
        epoch: number(epoch)?,
        seq: number(seq)?,
    })
}

fn number(digits: &str) -> Option<i64> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn cursor_text(cursor: Cursor) -> String {
    format!("{}.{}", cursor.epoch, cursor.seq)
}

fn item(item: &ChangeItem) -> Value {
    json!({
        "node_id": item.node_id,
        "parent_id": item.parent_id,
        "kind": kind(item.kind),
        "name": item.name,
        "state": state(item.state),
        "revision_id": item.revision_id,
        "size": item.size,
        "content_hash": item.content_hash.map(|hash| hash.to_string()),
        "seq": item.seq,
    })
}

/// A page ends on a `seq` boundary; an empty page after a hold still carries the cursor to poll from next.
pub(super) fn page(page: &ChangePage) -> Value {
    let items: Vec<Value> = page.items.iter().map(item).collect();
    json!({
        "items": items,
        "next_cursor": cursor_text(page.next_cursor),
        "has_more": page.has_more,
    })
}

pub(super) fn head(cursor: Cursor) -> Value {
    json!({"cursor": cursor_text(cursor)})
}

pub(super) const CHANGE_PAGE: SchemaRef = SchemaRef {
    name: "ChangePage",
    build: || {
        json!({
            "type": "object",
            "required": ["items", "next_cursor", "has_more"],
            "properties": {
                "items": {
                    "type": "array",
                    "description": "The latest state of each changed node, not an event; ascending by `seq`.",
                    "items": {
                        "type": "object",
                        "required": ["node_id", "kind", "name", "state", "seq"],
                        "properties": {
                            "node_id": {"type": "string", "format": "uuid"},
                            "parent_id": {"type": ["string", "null"], "format": "uuid"},
                            "kind": {"type": "string", "enum": ["file", "folder"]},
                            "name": {"type": "string"},
                            "state": {"type": "string", "enum": ["live", "tombstone"]},
                            "revision_id": {"type": ["string", "null"], "format": "uuid"},
                            "size": {"type": ["integer", "null"]},
                            "content_hash": {"type": ["string", "null"], "description": "64 hex digits."},
                            "seq": {"type": "integer"}
                        }
                    }
                },
                "next_cursor": {"type": "string", "description": "`E.S`: poll from here next."},
                "has_more": {"type": "boolean"}
            }
        })
    },
};

pub(super) const CHANGES_HEAD: SchemaRef = SchemaRef {
    name: "ChangesHead",
    build: || {
        json!({
            "type": "object",
            "required": ["cursor"],
            "properties": {"cursor": {"type": "string", "description": "`E.S`."}}
        })
    },
};
