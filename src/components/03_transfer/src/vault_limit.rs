//! The vault file size limit (v1.2, E-30/c01, AZ-3927): a file in a vault may hold at most `max_vault_file_bytes`.
//! Membership is 02's answer, never the client's: `start_upload` reads the parent's `NodeView::vault` and refuses a
//! larger declared size before any block is sent; every commit checks again in its effect on the entry 02 publishes
//! (`PublishedEntry::vault` and `size`), so a commit into a vault folder other than the start's parent, an
//! `Existing` target and the batch path are all covered.

use contracts::{EffectError, PublishedEntry};
use sc_error::ErrorCode;

/// The entry failure of a vault file above the limit; `commit_upload` answers it as `VaultFileTooLarge`.
pub(crate) const VAULT_FILE_TOO_LARGE: ErrorCode = ErrorCode::VaultFileTooLarge;

/// Whether a file of `size` bytes may sit in a vault whose limit is `max` bytes: at most `max`.
pub(crate) fn within_vault_limit(size: i64, max: u64) -> bool {
    u64::try_from(size).is_ok_and(|size| size <= max)
}

/// The commit-time check, first in a commit effect's `on_entry`: it fails only that entry.
pub(crate) fn check_entry(entry: &PublishedEntry, max: u64) -> Result<(), EffectError> {
    if entry.vault && !within_vault_limit(entry.size, max) {
        Err(EffectError::Entry(VAULT_FILE_TOO_LARGE))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sc_ids::FileHash;
    use uuid::Uuid;

    use super::*;

    const LIMIT: u64 = 104_857_600;

    fn entry(vault: bool, size: i64) -> PublishedEntry {
        PublishedEntry {
            node_id: Uuid::nil(),
            revision_id: Uuid::nil(),
            content_hash: FileHash::from_bytes([1; 32]),
            media_type: None,
            size,
            import_hint: false,
            vault,
        }
    }

    #[test]
    fn within_vault_limit_accepts_the_limit_and_refuses_one_byte_more() {
        // Arrange
        let (at_limit, above) = (104_857_600, 104_857_601);

        // Act
        let checked = (
            within_vault_limit(at_limit, LIMIT),
            within_vault_limit(above, LIMIT),
        );

        // Assert
        assert_eq!(checked, (true, false));
    }

    #[test]
    fn check_entry_refuses_a_vault_entry_above_the_limit_with_the_vault_code() {
        // Arrange
        let above = entry(true, 104_857_601);

        // Act
        let checked = check_entry(&above, LIMIT);

        // Assert
        assert_eq!(
            checked,
            Err(EffectError::Entry(ErrorCode::VaultFileTooLarge))
        );
    }

    #[test]
    fn check_entry_passes_a_vault_entry_at_the_limit_and_any_entry_outside_a_vault() {
        // Arrange
        let entries = [entry(true, 104_857_600), entry(false, 104_857_601)];

        // Act
        let checked = entries.map(|entry| check_entry(&entry, LIMIT));

        // Assert
        assert_eq!(checked, [Ok(()), Ok(())]);
    }
}
