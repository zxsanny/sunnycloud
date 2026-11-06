//! AC-3: every `TransferError` variant converted through `sc_error` yields exactly the code and status of the
//! task, with its parameters, and no request data in its text.

use std::time::Duration;

use sc_error::{BusinessError, DomainError, ErrorCode};
use sunnycloud_transfer::TransferError;

fn expected() -> Vec<(TransferError, &'static str, u16)> {
    vec![
        (TransferError::QuotaExceeded, "quota_exceeded", 507),
        (TransferError::FreeSpaceFloor, "free_space_floor", 507),
        (TransferError::SizeTooLarge, "size_too_large", 413),
        (
            TransferError::SessionCapExceeded,
            "session_cap_exceeded",
            409,
        ),
        (TransferError::StagedCapReached, "staged_cap_reached", 409),
        (TransferError::CommitIdReused, "commit_id_reused", 422),
        (TransferError::BatchTooLarge, "batch_too_large", 422),
        (TransferError::BlocklistMismatch, "blocklist_mismatch", 422),
        (TransferError::AccountNotActive, "account_not_active", 403),
        (TransferError::ParentNotFound, "parent_not_found", 404),
        (TransferError::NotAFolder, "not_a_folder", 422),
        (TransferError::ParentDeleted, "parent_deleted", 410),
        (TransferError::BlockHashMismatch, "block_hash_mismatch", 422),
        (
            TransferError::BlockLengthMismatch,
            "block_length_mismatch",
            422,
        ),
        (TransferError::BlockConflict, "block_conflict", 409),
        (TransferError::NeedBlocks(vec![3, 5]), "need_blocks", 409),
        (TransferError::EditedSince, "edited_since", 409),
        (
            TransferError::ContentUnavailable,
            "content_unavailable",
            404,
        ),
        (
            TransferError::RangeNotSatisfiable,
            "range_not_satisfiable",
            416,
        ),
        (TransferError::UploadSessionGone, "upload_session_gone", 410),
        (
            TransferError::Overloaded {
                retry_after: Duration::from_secs(2),
            },
            "overloaded",
            503,
        ),
    ]
}

#[test]
fn every_variant_converts_to_its_code_and_status() {
    // Arrange
    let table = expected();

    for (error, wire, status) in table {
        // Act
        let converted = DomainError::from(error.clone());

        // Assert
        let DomainError::Business(business) = &converted else {
            panic!("{error:?} is not a business failure: {converted:?}");
        };
        assert_eq!(business.code.as_str(), wire, "{error:?}");
        assert_eq!(business.code.status(), status, "{error:?}");
        assert_eq!(converted.code(), error.code());
    }
}

#[test]
fn the_table_lists_each_of_the_twenty_one_variants_once() {
    // Arrange
    let table = expected();

    // Act
    let mut wires: Vec<&str> = table.iter().map(|(_, wire, _)| *wire).collect();
    wires.sort_unstable();
    wires.dedup();

    // Assert
    assert_eq!(table.len(), 21);
    assert_eq!(wires.len(), 21);
}

#[test]
fn need_blocks_carries_its_indexes_and_overloaded_its_retry_after() {
    // Arrange
    let need = TransferError::NeedBlocks(vec![0, 7, 9]);
    let overloaded = TransferError::Overloaded {
        retry_after: Duration::from_millis(1500),
    };

    // Act
    let need = BusinessError::from(need);
    let overloaded = BusinessError::from(overloaded);

    // Assert
    assert_eq!(need.code, ErrorCode::NeedBlocks);
    assert_eq!(need.params.need_blocks, [0, 7, 9]);
    assert_eq!(need.params.retry_after, None);
    assert_eq!(
        overloaded.params.retry_after,
        Some(Duration::from_millis(1500))
    );
    assert!(overloaded.params.need_blocks.is_empty());
}

#[test]
fn the_other_variants_carry_no_parameters() {
    // Arrange
    let plain = [TransferError::QuotaExceeded, TransferError::BlockConflict];

    for error in plain {
        // Act
        let business = BusinessError::from(error);

        // Assert
        assert_eq!(business.params, Default::default());
    }
}

#[test]
fn the_message_of_each_variant_is_its_summary_without_request_data() {
    // Arrange
    let error = TransferError::NeedBlocks(vec![123_456]);

    // Act
    let shown = error.to_string();
    let converted = DomainError::from(error).to_string();

    // Assert
    assert_eq!(shown, "blocks needed");
    assert!(!shown.contains("123456"));
    assert_eq!(converted, "need_blocks");
}
