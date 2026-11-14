//! The pure parts of `commit_batch` (`commit_service.md`): the request hash a receipt is matched by, the entry
//! results in request order (entries refused before `publish` merged with `publish`'s results) and the stored
//! form of a `BatchResult` (JSON in `commit_receipts.response`, and the bytes of `Begin::Replay`).
//!
//! The request hash covers the entries in the order sent (owner decision 2026-11-14): a reordered batch is a
//! different request. Every field of an entry counts, each length-prefixed or tagged so that no two requests
//! encode alike. The commit id is not part of it: it is the receipt's key.

use contracts::{
    BatchEntry, BatchEntryResult, BatchResult, Committed, EntryError, EntryFailure, EntryResult,
};
use sc_error::ErrorCode;
use sc_ids::{Hash32, hash_block};
use serde_json::{Map, Value, json};
use thiserror::Error;
use uuid::Uuid;

/// Why a stored response or a result list cannot be turned into a `BatchResult`: 03 or 02 broke its own contract.
#[derive(Debug, Error)]
pub(crate) enum ResponseFault {
    #[error("stored batch response is not valid: {0}")]
    Stored(&'static str),
    #[error("publish returned {got} results for {expected} entries")]
    ResultCount { expected: usize, got: usize },
}

/// The hash of each entry's encoding, then the hash of the entry count and those hashes in order. Each piece
/// stays far below one block, as `hash_block` expects.
pub(crate) fn request_hash(entries: &[BatchEntry]) -> Hash32 {
    let count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    let mut all = Vec::with_capacity(4 + 32 * entries.len());
    all.extend_from_slice(&count.to_le_bytes());
    for entry in entries {
        all.extend_from_slice(hash_block(&encode_entry(entry)).as_bytes());
    }
    hash_block(&all)
}

fn encode_entry(entry: &BatchEntry) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + entry.name.len());
    out.extend_from_slice(entry.staged_key.as_bytes());
    out.extend_from_slice(entry.parent_id.as_bytes());
    put_text(&mut out, &entry.name);
    match entry.base_revision {
        Some(base) => {
            out.push(1);
            out.extend_from_slice(base.as_bytes());
        }
        None => out.push(0),
    }
    match entry.client_mtime {
        Some(mtime) => {
            out.push(1);
            out.extend_from_slice(&mtime.as_unix_micros().to_le_bytes());
        }
        None => out.push(0),
    }
    match &entry.client_meta {
        Some(meta) => {
            out.push(1);
            put_text(&mut out, meta);
        }
        None => out.push(0),
    }
    out.push(u8::from(entry.import_hint));
    out
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u64).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

/// The answer of each request entry, in order: `refused[i]` is `Some` for an entry refused before `publish`,
/// and each `None` takes the next of `published` (one per entry handed to `publish`, in order).
pub(crate) fn assemble<'a>(
    refused: &[Option<EntryError>],
    published: impl ExactSizeIterator<Item = &'a EntryResult>,
) -> Result<BatchResult, ResponseFault> {
    let expected = refused.iter().filter(|slot| slot.is_none()).count();
    if published.len() != expected {
        return Err(ResponseFault::ResultCount {
            expected,
            got: published.len(),
        });
    }
    let mut published = published;
    let per_entry = refused
        .iter()
        .map(|slot| match slot {
            Some(error) => BatchEntryResult::Failed(error.clone()),
            None => entry_result(published.next().expect("counted above")),
        })
        .collect();
    Ok(BatchResult { per_entry })
}

/// `commit_service.md` outcome and entry-error mapping. A staged row that is gone (or its blob row, which goes
/// with it) is `UploadNotFound`. 02's `NotFound` (the base revision's node vanished between the lookup and
/// `publish`) has no `EntryError` of its own and travels as `EffectFailed(not_found)`.
fn entry_result(result: &EntryResult) -> BatchEntryResult {
    match result {
        EntryResult::Published {
            node_id,
            revision_id,
            conflict_copy,
            ..
        } => BatchEntryResult::Committed(Committed {
            node_id: *node_id,
            revision_id: *revision_id,
            conflict_copy: *conflict_copy,
        }),
        EntryResult::Failed(failure) => BatchEntryResult::Failed(match failure {
            EntryFailure::NameInvalid(_) => EntryError::InvalidName,
            EntryFailure::ParentDeleted => EntryError::ParentGone,
            EntryFailure::ParentNotFound => EntryError::ParentNotFound,
            EntryFailure::NotAFolder => EntryError::NotAFolder,
            EntryFailure::BlocklistMismatch
            | EntryFailure::EffectFailed(ErrorCode::UploadSessionGone) => {
                EntryError::UploadNotFound
            }
            EntryFailure::NotFound => EntryError::EffectFailed(ErrorCode::NotFound),
            EntryFailure::EffectFailed(code) => EntryError::EffectFailed(*code),
        }),
    }
}

/// `[{"node_id", "revision_id", "conflict_copy"} | {"error", "code"?}, ...]`.
pub(crate) fn encode(result: &BatchResult) -> String {
    let items: Vec<Value> = result
        .per_entry
        .iter()
        .map(|entry| match entry {
            BatchEntryResult::Committed(committed) => json!({
                "node_id": committed.node_id.to_string(),
                "revision_id": committed.revision_id.to_string(),
                "conflict_copy": committed.conflict_copy.map(|id| id.to_string()),
            }),
            BatchEntryResult::Failed(EntryError::EffectFailed(code)) => {
                json!({ "error": "effect_failed", "code": code.as_str() })
            }
            BatchEntryResult::Failed(error) => json!({ "error": error_name(error) }),
        })
        .collect();
    Value::Array(items).to_string()
}

pub(crate) fn decode(stored: &[u8]) -> Result<BatchResult, ResponseFault> {
    let value: Value =
        serde_json::from_slice(stored).map_err(|_| ResponseFault::Stored("not JSON"))?;
    let items = value
        .as_array()
        .ok_or(ResponseFault::Stored("not an array"))?;
    let per_entry = items
        .iter()
        .map(|item| {
            let item = item
                .as_object()
                .ok_or(ResponseFault::Stored("item not an object"))?;
            match item.get("error") {
                Some(name) => decode_error(name, item).map(BatchEntryResult::Failed),
                None => Ok(BatchEntryResult::Committed(Committed {
                    node_id: id(item.get("node_id"))?,
                    revision_id: id(item.get("revision_id"))?,
                    conflict_copy: match item.get("conflict_copy") {
                        None | Some(Value::Null) => None,
                        some => Some(id(some)?),
                    },
                })),
            }
        })
        .collect::<Result<_, _>>()?;
    Ok(BatchResult { per_entry })
}

const ERROR_NAMES: [(&str, EntryError); 6] = [
    ("invalid_name", EntryError::InvalidName),
    ("parent_gone", EntryError::ParentGone),
    ("parent_not_found", EntryError::ParentNotFound),
    ("not_a_folder", EntryError::NotAFolder),
    ("blocklist_mismatch", EntryError::BlocklistMismatch),
    ("upload_not_found", EntryError::UploadNotFound),
];

fn error_name(error: &EntryError) -> &'static str {
    ERROR_NAMES
        .iter()
        .find(|(_, known)| known == error)
        .map_or("effect_failed", |(name, _)| name)
}

fn decode_error(name: &Value, item: &Map<String, Value>) -> Result<EntryError, ResponseFault> {
    let name = name
        .as_str()
        .ok_or(ResponseFault::Stored("error not a string"))?;
    if name == "effect_failed" {
        let code = item
            .get("code")
            .and_then(Value::as_str)
            .and_then(ErrorCode::from_wire)
            .ok_or(ResponseFault::Stored("unknown effect code"))?;
        return Ok(EntryError::EffectFailed(code));
    }
    ERROR_NAMES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, error)| error.clone())
        .ok_or(ResponseFault::Stored("unknown entry error"))
}

fn id(value: Option<&Value>) -> Result<Uuid, ResponseFault> {
    value
        .and_then(Value::as_str)
        .and_then(|text| Uuid::parse_str(text).ok())
        .ok_or(ResponseFault::Stored("id not a UUID"))
}

#[cfg(test)]
mod tests {
    use contracts::PublishOutcome;
    use sc_clock::Timestamp;

    use super::*;

    fn entry(name: &str) -> BatchEntry {
        BatchEntry {
            staged_key: Uuid::from_u128(1),
            parent_id: Uuid::from_u128(2),
            name: name.to_owned(),
            base_revision: None,
            client_mtime: None,
            client_meta: None,
            import_hint: false,
        }
    }

    fn batch() -> Vec<BatchEntry> {
        vec![entry("a.txt"), entry("b.txt"), entry("c.txt")]
    }

    #[test]
    fn request_hash_the_same_request_gives_the_same_hash() {
        // Arrange
        let (first, second) = (batch(), batch());

        // Act
        let hashes = (request_hash(&first), request_hash(&second));

        // Assert
        assert_eq!(hashes.0, hashes.1);
    }

    #[test]
    fn request_hash_any_changed_field_or_order_gives_another_hash() {
        // Arrange
        let original = request_hash(&batch());
        let changes: [fn(&mut Vec<BatchEntry>); 9] = [
            |b| b[1].name = "b2.txt".to_owned(),
            |b| b[1].staged_key = Uuid::from_u128(9),
            |b| b[1].parent_id = Uuid::from_u128(9),
            |b| b[1].base_revision = Some(Uuid::from_u128(9)),
            |b| b[1].client_mtime = Some(Timestamp::from_unix_micros(5)),
            |b| b[1].client_meta = Some("{}".to_owned()),
            |b| b[1].import_hint = true,
            |b| b.swap(0, 2),
            |b| {
                b.pop();
            },
        ];

        for (index, change) in changes.iter().enumerate() {
            let mut changed = batch();
            change(&mut changed);

            // Act
            let hash = request_hash(&changed);

            // Assert
            assert_ne!(hash, original, "change {index}");
        }
    }

    #[test]
    fn request_hash_field_boundaries_do_not_collide() {
        // Arrange
        let mut joined = batch();
        joined[0].name = "ab".to_owned();
        joined[0].client_meta = Some("c".to_owned());
        let mut split = batch();
        split[0].name = "a".to_owned();
        split[0].client_meta = Some("bc".to_owned());

        // Act
        let hashes = (request_hash(&joined), request_hash(&split));

        // Assert
        assert_ne!(hashes.0, hashes.1);
    }

    fn published(node: u128, copy: Option<u128>) -> EntryResult {
        EntryResult::Published {
            outcome: PublishOutcome::Created,
            node_id: Uuid::from_u128(node),
            revision_id: Uuid::from_u128(node + 100),
            conflict_copy: copy.map(Uuid::from_u128),
        }
    }

    #[test]
    fn assemble_merges_refused_entries_with_publish_results_in_request_order() {
        // Arrange
        let refused = [None, Some(EntryError::UploadNotFound), None, None];
        let results = [
            published(1, None),
            EntryResult::Failed(EntryFailure::NameInvalid("bad".to_owned())),
            EntryResult::Failed(EntryFailure::EffectFailed(ErrorCode::UploadSessionGone)),
        ];

        // Act
        let assembled = assemble(&refused, results.iter()).expect("assembled");

        // Assert
        assert_eq!(
            assembled.per_entry,
            vec![
                BatchEntryResult::Committed(Committed {
                    node_id: Uuid::from_u128(1),
                    revision_id: Uuid::from_u128(101),
                    conflict_copy: None,
                }),
                BatchEntryResult::Failed(EntryError::UploadNotFound),
                BatchEntryResult::Failed(EntryError::InvalidName),
                BatchEntryResult::Failed(EntryError::UploadNotFound),
            ]
        );
    }

    #[test]
    fn assemble_refuses_a_result_count_that_does_not_match() {
        // Arrange
        let refused = [None, None];
        let results = [published(1, None)];

        // Act
        let assembled = assemble(&refused, results.iter());

        // Assert
        assert!(matches!(
            assembled,
            Err(ResponseFault::ResultCount {
                expected: 2,
                got: 1
            })
        ));
    }

    #[test]
    fn encode_and_decode_round_trip_every_entry_kind() {
        // Arrange
        let mut per_entry = vec![
            entry_result(&published(1, None)),
            entry_result(&published(2, Some(7))),
            BatchEntryResult::Failed(EntryError::EffectFailed(ErrorCode::NotFound)),
        ];
        per_entry.extend(
            ERROR_NAMES
                .iter()
                .map(|(_, error)| BatchEntryResult::Failed(error.clone())),
        );
        let result = BatchResult { per_entry };

        // Act
        let decoded = decode(encode(&result).as_bytes()).expect("decoded");

        // Assert
        assert_eq!(decoded, result);
    }

    #[test]
    fn decode_refuses_malformed_stored_responses() {
        // Arrange
        let malformed: [&[u8]; 5] = [
            b"not json",
            b"{}",
            b"[{\"error\": \"no_such_error\"}]",
            b"[{\"error\": \"effect_failed\", \"code\": \"nope\"}]",
            b"[{\"node_id\": \"x\", \"revision_id\": \"y\"}]",
        ];

        for stored in malformed {
            // Act
            let decoded = decode(stored);

            // Assert
            assert!(
                matches!(decoded, Err(ResponseFault::Stored(_))),
                "{stored:?}"
            );
        }
    }
}
