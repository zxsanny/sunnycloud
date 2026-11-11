//! The JSON of the transfer routes. 03 fixes the ports, not the bodies; the names follow the DTOs the clients
//! already hold (`outcome`, `need_blocks`, `lane_limit`, `entries`, `status`, `parent_id` in the requests as 03
//! names it) and add what the ports carry beyond them.

use contracts::{
    BatchCommit, BatchEntry, BatchEntryResult, BatchResult, CommitUpload, Committed, Staged,
    StartUpload, StartUploadOutcome, UploadState,
};
use sc_ids::{FileHash, Hash32};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::json_body::{self, meta_text, optional, required, text, timestamp, uuid};
use crate::port_error::entry_error_code;
use crate::registry::SchemaRef;
use crate::registry::request::digest;
use crate::timestamp::rfc3339;

fn hash32(value: &Value) -> Option<Hash32> {
    digest(value.as_str()?).map(Hash32::from_bytes)
}

fn file_hash(value: &Value) -> Option<FileHash> {
    digest(value.as_str()?).map(FileHash::from_bytes)
}

fn size(value: &Value) -> Option<i64> {
    value.as_i64().filter(|size| *size >= 0)
}

fn import_hint(members: &Map<String, Value>) -> Option<bool> {
    Some(optional(members, "import_hint", Value::as_bool)?.unwrap_or(false))
}

/// `{parent_id, name, declared_size, client_key, content_hash?, base_revision?, client_meta?}`; an unknown
/// member, a wrong type or a negative size is `None`. A `null` is the same as an absent optional member.
pub(super) fn start_upload(body: &Value) -> Option<StartUpload> {
    let members = json_body::object(
        body,
        &[
            "parent_id",
            "name",
            "declared_size",
            "client_key",
            "content_hash",
            "base_revision",
            "client_meta",
        ],
    )?;
    Some(StartUpload {
        parent_id: required(members, "parent_id", uuid)?,
        name: required(members, "name", text)?,
        declared_size: required(members, "declared_size", size)?,
        client_key: required(members, "client_key", text)?,
        content_hash: optional(members, "content_hash", file_hash)?,
        base_revision: optional(members, "base_revision", uuid)?,
        client_meta: optional(members, "client_meta", meta_text)?,
    })
}

/// `{upload_id, parent_id, name, blocklist: [64 hex], base_revision?, client_mtime?, client_meta?, import_hint?}`.
pub(super) fn commit_upload(body: &Value) -> Option<CommitUpload> {
    let members = json_body::object(
        body,
        &[
            "upload_id",
            "parent_id",
            "name",
            "blocklist",
            "base_revision",
            "client_mtime",
            "client_meta",
            "import_hint",
        ],
    )?;
    let blocklist = required(members, "blocklist", |list| {
        list.as_array()?.iter().map(hash32).collect()
    })?;
    Some(CommitUpload {
        upload_id: required(members, "upload_id", uuid)?,
        parent_id: required(members, "parent_id", uuid)?,
        name: required(members, "name", text)?,
        base_revision: optional(members, "base_revision", uuid)?,
        blocklist,
        client_mtime: optional(members, "client_mtime", timestamp)?,
        client_meta: optional(members, "client_meta", meta_text)?,
        import_hint: import_hint(members)?,
    })
}

/// `{commit_id, entries: [{staged_key, parent_id, name, base_revision?, client_mtime?, client_meta?,
/// import_hint?}]}`. How many entries are too many is 03's to say.
pub(super) fn batch_commit(body: &Value) -> Option<BatchCommit> {
    let members = json_body::object(body, &["commit_id", "entries"])?;
    let entries = required(members, "entries", |list| {
        list.as_array()?.iter().map(batch_entry).collect()
    })?;
    Some(BatchCommit {
        commit_id: required(members, "commit_id", uuid)?,
        entries,
    })
}

fn batch_entry(entry: &Value) -> Option<BatchEntry> {
    let members = json_body::object(
        entry,
        &[
            "staged_key",
            "parent_id",
            "name",
            "base_revision",
            "client_mtime",
            "client_meta",
            "import_hint",
        ],
    )?;
    Some(BatchEntry {
        staged_key: required(members, "staged_key", uuid)?,
        parent_id: required(members, "parent_id", uuid)?,
        name: required(members, "name", text)?,
        base_revision: optional(members, "base_revision", uuid)?,
        client_mtime: optional(members, "client_mtime", timestamp)?,
        client_meta: optional(members, "client_meta", meta_text)?,
        import_hint: import_hint(members)?,
    })
}

pub(super) fn start_outcome(outcome: &StartUploadOutcome) -> Value {
    match outcome {
        StartUploadOutcome::Started(started) => json!({
            "outcome": "session",
            "upload_id": started.upload_id,
            "need_blocks": started.need_blocks,
            "block_size": started.block_size,
            "lane_limit": started.lane_limit,
            "expires_at": rfc3339(started.expires_at),
        }),
        StartUploadOutcome::NothingToSend { revision } => {
            json!({"outcome": "nothing_to_send", "revision_id": revision})
        }
    }
}

pub(super) fn upload_state(upload_id: Uuid, state: &UploadState) -> Value {
    json!({
        "upload_id": upload_id,
        "need_blocks": state.need_blocks,
        "received_count": state.received_count,
        "expires_at": rfc3339(state.expires_at),
    })
}

pub(super) fn staged(staged: &Staged) -> Value {
    json!({"staged_key": staged.staged_key, "size": staged.size})
}

/// A commit that lost the race to a newer edit says which node holds the copy of the replaced content.
pub(super) fn committed(committed: &Committed) -> Value {
    match committed.conflict_copy {
        Some(copy) => json!({
            "outcome": "conflict_copy",
            "node_id": committed.node_id,
            "revision_id": committed.revision_id,
            "copy_node_id": copy,
        }),
        None => json!({
            "outcome": "published",
            "node_id": committed.node_id,
            "revision_id": committed.revision_id,
        }),
    }
}

/// One result per entry, in entry order: the committed ids, or the wire code of the entry's failure.
pub(super) fn batch_result(result: &BatchResult) -> Value {
    let entries: Vec<Value> = result
        .per_entry
        .iter()
        .map(|entry| match entry {
            BatchEntryResult::Committed(done) => json!({
                "status": "committed",
                "node_id": done.node_id,
                "revision_id": done.revision_id,
                "conflict_copy": done.conflict_copy,
            }),
            BatchEntryResult::Failed(error) => {
                json!({"status": "entry_error", "code": entry_error_code(error).as_str()})
            }
        })
        .collect();
    json!({"entries": entries})
}

const UUID: fn() -> Value = || json!({"type": "string", "format": "uuid"});
const CLIENT_META: fn() -> Value = || json!({"description": "Any JSON value; stored as given."});
const CLIENT_MTIME: fn() -> Value = || json!({"type": "string", "format": "date-time"});

fn entry_properties() -> Value {
    json!({
        "parent_id": UUID(),
        "name": {"type": "string"},
        "base_revision": UUID(),
        "client_mtime": CLIENT_MTIME(),
        "client_meta": CLIENT_META(),
        "import_hint": {"type": "boolean"}
    })
}

pub(super) const START_REQUEST: SchemaRef = SchemaRef {
    name: "StartUploadRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["parent_id", "name", "declared_size", "client_key"],
            "additionalProperties": false,
            "properties": {
                "parent_id": UUID(),
                "name": {"type": "string"},
                "declared_size": {"type": "integer", "minimum": 0},
                "client_key": {"type": "string", "description": "Opaque; the same key resumes the same upload."},
                "content_hash": {"type": "string", "description": "64 hex digits."},
                "base_revision": UUID(),
                "client_meta": CLIENT_META()
            }
        })
    },
};

pub(super) const START_RESPONSE: SchemaRef = SchemaRef {
    name: "StartUploadOutcome",
    build: || {
        json!({
            "type": "object",
            "required": ["outcome"],
            "properties": {
                "outcome": {"type": "string", "enum": ["session", "nothing_to_send"]},
                "upload_id": UUID(),
                "need_blocks": {"type": "array", "items": {"type": "integer", "minimum": 0}},
                "block_size": {"type": "integer"},
                "lane_limit": {"type": "integer"},
                "expires_at": {"type": "string", "format": "date-time"},
                "revision_id": UUID()
            }
        })
    },
};

pub(super) const UPLOAD_STATE: SchemaRef = SchemaRef {
    name: "UploadState",
    build: || {
        json!({
            "type": "object",
            "required": ["upload_id", "need_blocks", "received_count", "expires_at"],
            "properties": {
                "upload_id": UUID(),
                "need_blocks": {"type": "array", "items": {"type": "integer", "minimum": 0}},
                "received_count": {"type": "integer", "minimum": 0},
                "expires_at": {"type": "string", "format": "date-time"}
            }
        })
    },
};

pub(super) const STAGED: SchemaRef = SchemaRef {
    name: "Staged",
    build: || {
        json!({
            "type": "object",
            "required": ["staged_key", "size"],
            "properties": {"staged_key": UUID(), "size": {"type": "integer", "minimum": 0}}
        })
    },
};

pub(super) const COMMIT_REQUEST: SchemaRef = SchemaRef {
    name: "CommitUploadRequest",
    build: || {
        let mut properties = entry_properties();
        if let Some(object) = properties.as_object_mut() {
            object.insert("upload_id".to_owned(), UUID());
            object.insert(
                "blocklist".to_owned(),
                json!({"type": "array", "items": {"type": "string", "description": "64 hex digits."}}),
            );
        }
        json!({
            "type": "object",
            "required": ["upload_id", "parent_id", "name", "blocklist"],
            "additionalProperties": false,
            "properties": properties
        })
    },
};

pub(super) const COMMITTED: SchemaRef = SchemaRef {
    name: "Committed",
    build: || {
        json!({
            "type": "object",
            "required": ["outcome", "node_id", "revision_id"],
            "properties": {
                "outcome": {"type": "string", "enum": ["published", "conflict_copy"]},
                "node_id": UUID(),
                "revision_id": UUID(),
                "copy_node_id": UUID()
            }
        })
    },
};

pub(super) const BATCH_REQUEST: SchemaRef = SchemaRef {
    name: "BatchCommitRequest",
    build: || {
        let mut properties = entry_properties();
        if let Some(object) = properties.as_object_mut() {
            object.insert("staged_key".to_owned(), UUID());
        }
        json!({
            "type": "object",
            "required": ["commit_id", "entries"],
            "additionalProperties": false,
            "properties": {
                "commit_id": UUID(),
                "entries": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["staged_key", "parent_id", "name"],
                        "additionalProperties": false,
                        "properties": properties
                    }
                }
            }
        })
    },
};

pub(super) const BATCH_RESULT: SchemaRef = SchemaRef {
    name: "BatchCommitResult",
    build: || {
        json!({
            "type": "object",
            "required": ["entries"],
            "properties": {
                "entries": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["status"],
                        "properties": {
                            "status": {"type": "string", "enum": ["committed", "entry_error"]},
                            "node_id": UUID(),
                            "revision_id": UUID(),
                            "conflict_copy": {"type": ["string", "null"], "format": "uuid"},
                            "code": {"type": "string"}
                        }
                    }
                }
            }
        })
    },
};
