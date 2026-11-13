//! AZ-3734 AC-3 and AC-5 (IT-03-1): every refusal of a start is typed and leaves the reservation, the rows and
//! the volume as they were.

use contracts::UploadError;
use uuid::Uuid;

use crate::start_ports::CreateFault;
use crate::start_support::{GIB, MIB, Rig, close, file, hash, node, request, started};

/// Reserved bytes, session rows, uploading blob rows and files.
async fn footprint(rig: &Rig) -> (i64, i64, i64, usize) {
    (
        rig.reserved().await,
        rig.sessions().await,
        rig.uploading_blobs().await,
        rig.files(),
    )
}

#[tokio::test]
async fn a_file_above_the_declared_size_ceiling_is_too_large() {
    // Arrange
    let rig = Rig::new().await;
    let before = footprint(&rig).await;

    // Act
    let result = rig.start(request(rig.root, "big", 51 * GIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::SizeTooLarge)),
        "{result:?}"
    );
    assert_eq!(footprint(&rig).await, before);
    rig.finish().await;
}

#[tokio::test]
async fn a_file_above_the_remaining_quota_is_quota_exceeded() {
    // Arrange
    let rig = Rig::new().await;
    let before = footprint(&rig).await;

    // Act
    let result = rig.start(request(rig.root, "big", 11 * GIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::QuotaExceeded)),
        "{result:?}"
    );
    assert_eq!(footprint(&rig).await, before);
    rig.finish().await;
}

#[tokio::test]
async fn a_ninth_open_session_is_refused_by_the_cap() {
    // Arrange
    let rig = Rig::new().await;
    for index in 0..8 {
        started(
            rig.start(request(
                rig.root,
                &format!("f{index}"),
                MIB,
                &format!("k{index}"),
            ))
            .await,
        );
    }
    let before = footprint(&rig).await;

    // Act
    let result = rig.start(request(rig.root, "ninth", MIB, "k8")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::SessionCapReached)),
        "{result:?}"
    );
    assert_eq!(footprint(&rig).await, before);
    rig.finish().await;
}

#[tokio::test]
async fn a_start_below_the_free_space_floor_is_refused_and_compensated() {
    // Arrange
    let rig = Rig::new().await;
    rig.writer.set(CreateFault::FreeSpaceFloor);
    let before = footprint(&rig).await;

    // Act
    let result = rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::FreeSpaceFloor)),
        "{result:?}"
    );
    assert_eq!(footprint(&rig).await, before);
    rig.finish().await;
}

#[tokio::test]
async fn a_name_with_a_slash_is_invalid() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let result = rig.start(request(rig.root, "a/b", MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::InvalidName)),
        "{result:?}"
    );
    assert_eq!(footprint(&rig).await, (0, 0, 0, 0));
    rig.finish().await;
}

#[tokio::test]
async fn a_missing_parent_is_not_found() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let result = rig.start(request(Uuid::new_v4(), "a.bin", MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::ParentNotFound)),
        "{result:?}"
    );
    assert_eq!(footprint(&rig).await, (0, 0, 0, 0));
    rig.finish().await;
}

#[tokio::test]
async fn a_file_as_parent_is_not_a_folder() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (parent, _) = file(&mut owner, rig.account, rig.root, "doc.txt", &hash(1)).await;
    close(owner).await;

    // Act
    let result = rig.start(request(parent, "a.bin", MIB, "k1")).await;

    // Assert
    assert!(matches!(result, Err(UploadError::NotAFolder)), "{result:?}");
    assert_eq!(rig.reserved().await, 0);
    assert_eq!(rig.sessions().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn a_deleted_parent_is_not_found() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let folder = node(&mut owner, rig.account, Some(rig.root), "folder", "gone").await;
    close(owner).await;
    rig.exec(&format!(
        "UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = '{folder}'"
    ))
    .await;

    // Act
    let result = rig.start(request(folder, "a.bin", MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::ParentNotFound)),
        "{result:?}"
    );
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn a_parent_of_another_account_is_not_found() {
    // Arrange
    let rig = Rig::new().await;
    let other = Uuid::new_v4();
    let mut owner = rig.owner().await;
    crate::support::account(&mut owner, other).await;
    let foreign_root = node(&mut owner, other, None, "folder", "").await;
    close(owner).await;

    // Act
    let result = rig.start(request(foreign_root, "a.bin", MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::ParentNotFound)),
        "{result:?}"
    );
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn a_zero_byte_large_upload_is_refused_without_a_reservation() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let result = rig.start(request(rig.root, "empty", 0, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::Unexpected(_))),
        "{result:?}"
    );
    assert_eq!(footprint(&rig).await, (0, 0, 0, 0));
    rig.finish().await;
}

/// AZ-4264 AC-3: a refused start drops its plan with nothing to release; the writer keeps no plan between
/// `plan` and `create_file` (04's `unused_plans_leave_nothing_in_the_writer_and_another_writer_creates_a_kept_plan`).
#[tokio::test]
async fn a_hundred_starts_refused_after_planning_leave_nothing_and_the_next_start_opens() {
    // Arrange
    let rig = Rig::new().await;
    let before = footprint(&rig).await;

    // Act
    let mut refused = 0;
    for index in 0..100 {
        let result = rig
            .start(request(rig.root, "big", 11 * GIB, &format!("k{index}")))
            .await;
        refused += usize::from(matches!(result, Err(UploadError::QuotaExceeded)));
    }
    let after_refusals = footprint(&rig).await;
    started(rig.start(request(rig.root, "a.bin", MIB, "next")).await);

    // Assert
    assert_eq!(refused, 100);
    assert_eq!(after_refusals, before);
    assert_eq!(footprint(&rig).await, (MIB, 1, 1, 1));
    rig.finish().await;
}
