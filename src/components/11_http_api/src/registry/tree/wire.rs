//! The JSON of the tree routes. 02 fixes the ports, not the bodies; the names follow the DTOs the clients
//! already hold (`node_id`, `current_revision`, `content_hash`, `next`) and add what the ports carry beyond them.

use contracts::{
    ListingPage, NodeKind, NodeState, NodeView, RevisionMatch, RevisionRef, RevisionView,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::json_body::{self, meta_value, optional, required, text, uuid};
use crate::registry::SchemaRef;
use crate::timestamp::rfc3339;

/// `listing_default_limit` of 02 (`tree_reader.md`): `TreeReader::list_children` takes a number, so 11 names
/// the default for a request without `limit`. A page of `find_revisions` has the same default.
pub(super) const DEFAULT_PAGE_LIMIT: u32 = 1000;

pub(super) struct CreateFolder {
    pub(super) parent_id: Uuid,
    pub(super) name: String,
}

/// `{parent_id, name}`.
pub(super) fn create_folder(body: &Value) -> Option<CreateFolder> {
    let members = json_body::object(body, &["parent_id", "name"])?;
    Some(CreateFolder {
        parent_id: required(members, "parent_id", uuid)?,
        name: required(members, "name", text)?,
    })
}

pub(super) struct UpdateNode {
    pub(super) name: Option<String>,
    pub(super) parent_id: Option<Uuid>,
}

/// `{name?, parent_id?}`; a `null` is the same as an absent member. 02 says what neither member means.
pub(super) fn update_node(body: &Value) -> Option<UpdateNode> {
    let members = json_body::object(body, &["name", "parent_id"])?;
    Some(UpdateNode {
        name: optional(members, "name", text)?,
        parent_id: optional(members, "parent_id", uuid)?,
    })
}

pub(super) fn node_view(node: &NodeView) -> Value {
    json!({
        "node_id": node.id,
        "parent_id": node.parent_id,
        "kind": kind(node.kind),
        "name": node.name,
        "state": state(node.state),
        "current_revision": node.current_revision_id,
        "size": node.size,
        "content_hash": node.content_hash.map(|hash| hash.to_string()),
        "media_type": node.media_type,
        "client_mtime": node.client_mtime.map(rfc3339),
        "created_at": rfc3339(node.created_at),
        "updated_at": rfc3339(node.updated_at),
        "seq": node.seq,
    })
}

pub(in crate::registry) fn kind(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "file",
        NodeKind::Folder => "folder",
    }
}

pub(in crate::registry) fn state(state: NodeState) -> &'static str {
    match state {
        NodeState::Live => "live",
        NodeState::Tombstone => "tombstone",
    }
}

/// `{items, next}`; `next` is the opaque key for `after`, `null` on the last page.
pub(super) fn listing(page: &ListingPage) -> Value {
    let items: Vec<Value> = page.items.iter().map(node_view).collect();
    json!({"items": items, "next": page.next})
}

/// A revision with its blocklist. The `client_meta` text is JSON by 02's contract; text that is not is 02's fault.
pub(super) fn revision_view(view: &RevisionView) -> Result<Value, serde_json::Error> {
    let blocklist: Option<Vec<String>> = view
        .blocklist
        .as_ref()
        .map(|hashes| hashes.iter().map(ToString::to_string).collect());
    Ok(json!({
        "revision_id": view.id,
        "node_id": view.node_id,
        "size": view.size,
        "content_hash": view.content_hash.to_string(),
        "blocklist": blocklist,
        "media_type": view.media_type,
        "client_mtime": view.client_mtime.map(rfc3339),
        "client_meta": meta_value(view.client_meta.as_deref())?,
        "created_at": rfc3339(view.created_at),
    }))
}

pub(super) fn revision_refs(found: &[RevisionRef]) -> Value {
    let items: Vec<Value> = found
        .iter()
        .map(|found| json!({"revision_id": found.revision_id, "node_id": found.node_id}))
        .collect();
    json!({"items": items})
}

pub(super) fn revision_matches(found: &[RevisionMatch]) -> Result<Value, serde_json::Error> {
    let items = found
        .iter()
        .map(|found| {
            Ok(json!({
                "revision_id": found.revision_id,
                "node_id": found.node_id,
                "content_hash": found.hash.to_string(),
                "client_meta": meta_value(found.client_meta.as_deref())?,
            }))
        })
        .collect::<Result<Vec<Value>, serde_json::Error>>()?;
    Ok(Value::Array(items))
}

fn node_schema() -> Value {
    json!({
        "type": "object",
        "required": ["node_id", "kind", "name", "state", "created_at", "updated_at", "seq"],
        "properties": {
            "node_id": {"type": "string", "format": "uuid"},
            "parent_id": {"type": ["string", "null"], "format": "uuid"},
            "kind": {"type": "string", "enum": ["file", "folder"]},
            "name": {"type": "string"},
            "state": {"type": "string", "enum": ["live", "tombstone"]},
            "current_revision": {"type": ["string", "null"], "format": "uuid"},
            "size": {"type": ["integer", "null"]},
            "content_hash": {"type": ["string", "null"], "description": "64 hex digits; files only."},
            "media_type": {"type": ["string", "null"]},
            "client_mtime": {"type": ["string", "null"], "format": "date-time"},
            "created_at": {"type": "string", "format": "date-time"},
            "updated_at": {"type": "string", "format": "date-time"},
            "seq": {"type": "integer"}
        }
    })
}

pub(super) const NODE_VIEW: SchemaRef = SchemaRef {
    name: "NodeView",
    build: node_schema,
};

pub(super) const NODE_LISTING: SchemaRef = SchemaRef {
    name: "NodeListing",
    build: || {
        json!({
            "type": "object",
            "required": ["items", "next"],
            "properties": {
                "items": {"type": "array", "items": node_schema()},
                "next": {"type": ["string", "null"], "description": "Pass as `after` for the next page."}
            }
        })
    },
};

pub(super) const REVISION_VIEW: SchemaRef = SchemaRef {
    name: "RevisionView",
    build: || {
        json!({
            "type": "object",
            "required": ["revision_id", "node_id", "size", "content_hash", "created_at"],
            "properties": {
                "revision_id": {"type": "string", "format": "uuid"},
                "node_id": {"type": "string", "format": "uuid"},
                "size": {"type": "integer"},
                "content_hash": {"type": "string", "description": "64 hex digits."},
                "blocklist": {"type": ["array", "null"], "items": {"type": "string"}},
                "media_type": {"type": ["string", "null"]},
                "client_mtime": {"type": ["string", "null"], "format": "date-time"},
                "client_meta": {},
                "created_at": {"type": "string", "format": "date-time"}
            }
        })
    },
};

pub(super) const REVISION_LIST: SchemaRef = SchemaRef {
    name: "RevisionList",
    build: || {
        json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["revision_id", "node_id"],
                        "properties": {
                            "revision_id": {"type": "string", "format": "uuid"},
                            "node_id": {"type": "string", "format": "uuid"}
                        }
                    }
                }
            }
        })
    },
};

pub(super) const REVISION_MATCHES: SchemaRef = SchemaRef {
    name: "RevisionMatches",
    build: || {
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "required": ["revision_id", "node_id", "content_hash"],
                "properties": {
                    "revision_id": {"type": "string", "format": "uuid"},
                    "node_id": {"type": "string", "format": "uuid"},
                    "content_hash": {"type": "string", "description": "64 hex digits."},
                    "client_meta": {}
                }
            }
        })
    },
};

pub(super) const CREATE_FOLDER_REQUEST: SchemaRef = SchemaRef {
    name: "CreateFolderRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["parent_id", "name"],
            "additionalProperties": false,
            "properties": {
                "parent_id": {"type": "string", "format": "uuid"},
                "name": {"type": "string"}
            }
        })
    },
};

pub(super) const UPDATE_NODE_REQUEST: SchemaRef = SchemaRef {
    name: "UpdateNodeRequest",
    build: || {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "name": {"type": "string"},
                "parent_id": {"type": "string", "format": "uuid"}
            }
        })
    },
};

pub(super) const LOOKUP_REQUEST: SchemaRef = SchemaRef {
    name: "RevisionLookupRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["content_hashes"],
            "additionalProperties": false,
            "properties": {
                "content_hashes": {
                    "type": "array",
                    "maxItems": 1000,
                    "items": {"type": "string", "description": "64 hex digits."}
                }
            }
        })
    },
};
