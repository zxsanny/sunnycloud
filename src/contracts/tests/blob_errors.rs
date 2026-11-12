//! Shapes of the 04 errors and key-bearing DTOs: exact variant sets and redacted `Debug`.

mod common;

use std::collections::BTreeSet;

use common::{fault, id};
use contracts::{
    BlobCatalogError, BlobError, BlobHandle, BlobPlan, BlobRegistryError, EpochMarkerError,
    GcQueueError, InventoryError, KeyError, MasterKey, ScrubError,
};

fn plan() -> BlobPlan {
    BlobPlan {
        blob_id: id(),
        wrapped_key: vec![222, 173, 190, 239],
        master_key_id: 1,
    }
}

fn handle() -> BlobHandle {
    BlobHandle {
        plan: plan(),
        plaintext_size: 10,
    }
}

#[test]
fn a_plan_a_handle_and_a_master_key_print_no_key_material() {
    // Arrange
    let planned = plan();
    let key = MasterKey(vec![200, 201, 202]);

    // Act
    let printed = format!("{planned:?} {:?} {key:?}", handle());

    // Assert
    assert!(printed.contains("<redacted>"), "{printed}");
    assert!(!printed.contains("222"), "{printed}");
    assert!(!printed.contains("200"), "{printed}");
}

fn blob_error_name(error: &BlobError) -> &'static str {
    match error {
        BlobError::NoSpace => "no_space",
        BlobError::FreeSpaceFloor => "free_space_floor",
        BlobError::Io(_) => "io",
        BlobError::BlobMissing => "blob_missing",
        BlobError::KeyUnavailable => "key_unavailable",
        BlobError::Corrupt => "corrupt",
        BlobError::RangeNotSatisfiable => "range_not_satisfiable",
        BlobError::OutOfRange => "out_of_range",
        BlobError::SizeMismatch => "size_mismatch",
        BlobError::BlockConflict => "block_conflict",
    }
}

fn key_error_name(error: &KeyError) -> &'static str {
    match error {
        KeyError::FileMissing => "file_missing",
        KeyError::WrongLength => "wrong_length",
        KeyError::KeyCheckMismatch => "key_check_mismatch",
        KeyError::KeyCheckAbsentButBlobsExist => "key_check_absent_but_blobs_exist",
        KeyError::KeyCheckSampleFailed { .. } => "key_check_sample_failed",
        KeyError::Unexpected(_) => "unexpected",
    }
}

#[test]
fn blob_error_has_exactly_the_ten_variants_of_the_contract() {
    // Arrange
    let all = [
        BlobError::NoSpace,
        BlobError::FreeSpaceFloor,
        BlobError::Io(fault()),
        BlobError::BlobMissing,
        BlobError::KeyUnavailable,
        BlobError::Corrupt,
        BlobError::RangeNotSatisfiable,
        BlobError::OutOfRange,
        BlobError::SizeMismatch,
        BlobError::BlockConflict,
    ];

    // Act
    let names: BTreeSet<_> = all.iter().map(blob_error_name).collect();

    // Assert
    assert_eq!(names.len(), 10);
}

#[test]
fn key_error_names_the_sampled_blob_on_a_failed_key_check() {
    // Arrange
    let error = KeyError::KeyCheckSampleFailed { blob_id: id() };

    // Act
    let name = key_error_name(&error);

    // Assert
    assert_eq!(name, "key_check_sample_failed");
}

#[test]
fn registry_catalog_gc_scrub_marker_and_inventory_errors_carry_one_variant_each() {
    // Arrange
    let registry = BlobRegistryError::Unexpected(fault());
    let catalog = BlobCatalogError::Unexpected(fault());
    let queue = GcQueueError::Unexpected(fault());
    let scrub = ScrubError::Unexpected(fault());
    let inventory = InventoryError::Unexpected(fault());
    let marker = EpochMarkerError::Io(fault());

    // Act: an irrefutable pattern compiles only while the enum has one variant
    let BlobRegistryError::Unexpected(_) = registry;
    let BlobCatalogError::Unexpected(_) = catalog;
    let GcQueueError::Unexpected(_) = queue;
    let ScrubError::Unexpected(_) = scrub;
    let InventoryError::Unexpected(_) = inventory;
    let EpochMarkerError::Io(_) = marker;

    // Assert
    assert_eq!(registry.to_string(), "unexpected failure");
    assert_eq!(catalog.to_string(), "unexpected failure");
    assert_eq!(queue.to_string(), "unexpected failure");
    assert_eq!(scrub.to_string(), "unexpected failure");
    assert_eq!(inventory.to_string(), "unexpected failure");
    assert_eq!(marker.to_string(), "epoch marker i/o failure");
}
