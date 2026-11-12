//! AC-1 planner matrix and AC-3 priority rule (AZ-3841), pure.

use contracts::PublishedEntry;
use media_shared::policy::CURRENT_PARAMS_VERSION;
use media_shared::types::JobKind;
use sc_ids::FileHash;
use uuid::Uuid;

use super::JobPlanner;
use crate::job_queue::Priority;

const THRESHOLD: u64 = 5;

fn planner() -> JobPlanner {
    JobPlanner::new(CURRENT_PARAMS_VERSION, THRESHOLD)
}

fn entry(media_type: Option<&str>) -> PublishedEntry {
    PublishedEntry {
        node_id: Uuid::from_u128(1),
        revision_id: Uuid::from_u128(2),
        content_hash: FileHash::from_bytes([7; 32]),
        media_type: media_type.map(str::to_owned),
        size: 100,
        import_hint: false,
        vault: false,
    }
}

fn kinds(entry: &PublishedEntry) -> Vec<JobKind> {
    planner()
        .plan(Uuid::from_u128(3), entry)
        .iter()
        .map(|spec| spec.kind)
        .collect()
}

#[test]
fn every_photo_type_gets_one_stills_job() {
    for media_type in [
        "image/jpeg",
        "image/png",
        "image/webp",
        "image/gif",
        "image/heic",
    ] {
        // Arrange
        let entry = entry(Some(media_type));

        // Act
        let planned = kinds(&entry);

        // Assert
        assert_eq!(planned, [JobKind::Stills], "{media_type}");
    }
}

#[test]
fn every_video_type_gets_a_poster_and_a_transcode_job() {
    for media_type in ["video/mp4", "video/quicktime", "video/x-matroska"] {
        // Arrange
        let entry = entry(Some(media_type));

        // Act
        let planned = kinds(&entry);

        // Assert
        assert_eq!(
            planned,
            [JobKind::Poster, JobKind::Transcode],
            "{media_type}"
        );
    }
}

#[test]
fn svg_unknown_unsupported_and_untyped_entries_get_none() {
    for media_type in [
        Some("image/svg+xml"),
        Some("application/octet-stream"),
        Some("video/webm"),
        Some("IMAGE/JPEG"),
        None,
    ] {
        // Arrange
        let entry = entry(media_type);

        // Act
        let planned = kinds(&entry);

        // Assert
        assert!(planned.is_empty(), "{media_type:?}");
    }
}

#[test]
fn a_vault_file_and_an_empty_file_get_none() {
    // Arrange
    let vault = PublishedEntry {
        vault: true,
        ..entry(Some("image/jpeg"))
    };
    let empty = PublishedEntry {
        size: 0,
        ..entry(Some("video/mp4"))
    };

    // Act
    let planned = (kinds(&vault), kinds(&empty));

    // Assert
    assert_eq!(planned, (vec![], vec![]));
}

#[test]
fn a_spec_carries_the_account_revision_hash_and_params_version() {
    // Arrange
    let entry = entry(Some("image/png"));
    let account = Uuid::from_u128(9);

    // Act
    let specs = JobPlanner::new(4, THRESHOLD).plan(account, &entry);

    // Assert
    assert_eq!(specs.len(), 1);
    let spec = specs[0];
    assert_eq!(
        (
            spec.account_id,
            spec.revision_id,
            spec.content_hash,
            spec.params_version
        ),
        (account, entry.revision_id, entry.content_hash, 4)
    );
}

#[test]
fn priority_is_interactive_at_the_threshold_and_bulk_above_it() {
    // Arrange
    let planner = planner();

    // Act
    let at_threshold = planner.priority(false, THRESHOLD);
    let above = planner.priority(false, THRESHOLD + 1);
    let none_queued = planner.priority(false, 0);

    // Assert
    assert_eq!(
        (at_threshold, above, none_queued),
        (Priority::Interactive, Priority::Bulk, Priority::Interactive)
    );
}

#[test]
fn the_import_hint_is_bulk_whatever_the_count() {
    // Arrange
    let planner = planner();

    // Act
    let hinted = planner.priority(true, 0);

    // Assert
    assert_eq!(hinted, Priority::Bulk);
}

#[test]
fn the_counting_probe_limit_is_threshold_plus_one() {
    // Arrange
    let planner = planner();

    // Act
    let limit = planner.count_limit();

    // Assert
    assert_eq!(limit, 6);
    assert_eq!(JobPlanner::new(1, u64::MAX).count_limit(), u64::MAX);
}
