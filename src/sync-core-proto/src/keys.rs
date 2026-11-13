//! Commit-ID rules: which key an upload sends, and what a batch id must not be.

use sc_ids::FileHash;
use thiserror::Error;
use uuid::Uuid;

/// Deterministic mobile `client_key`: `sc_ids::derive_client_key` over `(device_id, asset_id, file_hash)`.
pub fn client_key_for_mobile(device_id: Uuid, asset_id: &str, file_hash: &FileHash) -> Uuid {
    sc_ids::derive_client_key(device_id, asset_id, file_hash)
}

/// The stored key when its file hash equals `current`, otherwise a fresh one. Randomness and time enter
/// only through `fresh`.
pub fn reuse_or_new_key(
    stored: Option<(Uuid, FileHash)>,
    current: FileHash,
    fresh: impl FnOnce() -> Uuid,
) -> Uuid {
    match stored {
        Some((key, hash)) if hash == current => key,
        _ => fresh(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum BatchIdError {
    #[error("a batch commit id is a UUIDv7")]
    NotV7,
    #[error("a batch commit id equals the client key of one of its members")]
    EqualsMemberKey,
}

/// A batch `commit_id` is a UUIDv7 and equals no member key.
pub fn check_batch_ids(commit_id: Uuid, member_keys: &[Uuid]) -> Result<(), BatchIdError> {
    if commit_id.get_version_num() != 7 {
        return Err(BatchIdError::NotV7);
    }
    if member_keys.contains(&commit_id) {
        return Err(BatchIdError::EqualsMemberKey);
    }
    Ok(())
}
