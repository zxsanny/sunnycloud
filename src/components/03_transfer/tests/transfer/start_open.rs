//! AZ-3734 AC-1, AC-2, AC-4, AC-7, AC-8 (IT-03-1): a start opens a session and reserves its size, a repeat of
//! the key resumes it, the cap holds under 16 concurrent starts, a failed file creation leaves nothing behind and
//! an inactive account is refused.

use std::sync::Arc;

use contracts::{StartUploadOutcome, UploadError};
use sunnycloud_transfer::BLOCK_SIZE;

use crate::start_ports::CreateFault;
use crate::start_support::{MIB, Rig, TTL, now, request, started};

#[tokio::test]
async fn a_ten_mib_file_starts_a_session_and_reserves_its_size() {
    // Arrange
    let rig = Rig::new().await;
    let before = rig.reserved().await;

    // Act
    let session = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);

    // Assert
    assert_eq!(session.need_blocks, vec![0, 1, 2, 3, 4]);
    assert_eq!(session.block_size as usize, BLOCK_SIZE);
    assert_eq!(session.lane_limit, 4);
    assert_eq!(
        session.expires_at,
        now(&rig.clock).checked_add(TTL).expect("expiry")
    );
    assert_eq!(rig.reserved().await - before, 10 * MIB);
    assert_eq!(rig.sessions().await, 1);
    assert_eq!(rig.uploading_blobs().await, 1);
    assert_eq!(rig.files(), 1);
    rig.finish().await;
}

#[tokio::test]
async fn the_same_key_resumes_the_session_without_a_second_reservation() {
    // Arrange
    let rig = Rig::new().await;
    let first = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);
    let reserved = rig.reserved().await;

    // Act
    let again = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);

    // Assert
    assert_eq!(again, first);
    assert_eq!(rig.reserved().await, reserved);
    assert_eq!(rig.sessions().await, 1);
    rig.finish().await;
}

#[tokio::test]
async fn the_same_key_with_another_size_is_a_client_key_mismatch() {
    // Arrange
    let rig = Rig::new().await;
    started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);
    let reserved = rig.reserved().await;

    // Act
    let result = rig.start(request(rig.root, "a.bin", 11 * MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::ClientKeyMismatch)),
        "{result:?}"
    );
    assert_eq!(rig.reserved().await, reserved);
    assert_eq!(rig.sessions().await, 1);
    rig.finish().await;
}

#[tokio::test]
async fn another_key_starts_another_session() {
    // Arrange
    let rig = Rig::new().await;
    let first = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);

    // Act
    let second = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k2")).await);

    // Assert
    assert_ne!(second.upload_id, first.upload_id);
    assert_eq!(rig.reserved().await, 20 * MIB);
    assert_eq!(rig.sessions().await, 2);
    rig.finish().await;
}

#[tokio::test]
async fn a_resumed_session_needs_only_the_blocks_not_received() {
    // Arrange
    let rig = Rig::new().await;
    let first = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);
    rig.exec(&format!(
        "UPDATE upload_sessions SET received = '\\x05' WHERE id = '{}'",
        first.upload_id
    ))
    .await;

    // Act
    let again = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);

    // Assert
    assert_eq!(again.need_blocks, vec![1, 3, 4]);
    rig.finish().await;
}

#[tokio::test]
async fn sixteen_concurrent_starts_against_a_cap_of_eight_open_exactly_eight() {
    // Arrange
    let rig = Rig::new().await;
    let service = Arc::clone(&rig.service);
    let (account, root) = (rig.account, rig.root);

    // Act
    let tasks: Vec<_> = (0..16)
        .map(|index| {
            let service = Arc::clone(&service);
            tokio::spawn(async move {
                let request = request(root, &format!("f{index}"), MIB, &format!("key-{index}"));
                service.start_upload(account, request).await
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await.expect("start task"));
    }

    // Assert
    let opened = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(StartUploadOutcome::Started(_))))
        .count();
    let capped = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Err(UploadError::SessionCapReached)))
        .count();
    assert_eq!((opened, capped), (8, 8), "{outcomes:?}");
    assert_eq!(rig.sessions().await, 8);
    assert_eq!(rig.reserved().await, 8 * MIB);
    assert_eq!(rig.uploading_blobs().await, 8);
    drop(service);
    rig.finish().await;
}

#[tokio::test]
async fn a_failed_file_creation_leaves_no_rows_and_no_reservation() {
    // Arrange
    let rig = Rig::new().await;
    rig.writer.set(CreateFault::Io);
    let before = rig.reserved().await;

    // Act
    let result = rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await;

    // Assert
    assert!(matches!(result, Err(UploadError::Storage(_))), "{result:?}");
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.uploading_blobs().await, 0);
    assert_eq!(rig.reserved().await, before);
    assert_eq!(rig.files(), 0);
    rig.finish().await;
}

#[tokio::test]
async fn a_start_after_a_compensated_failure_with_the_same_key_opens_a_session() {
    // Arrange
    let rig = Rig::new().await;
    rig.writer.set(CreateFault::Io);
    let failed = rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await;
    rig.writer.set(CreateFault::None);

    // Act
    let session = started(rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await);

    // Assert
    assert!(failed.is_err());
    assert_eq!(session.need_blocks.len(), 5);
    assert_eq!(rig.reserved().await, 10 * MIB);
    rig.finish().await;
}

#[tokio::test]
async fn an_account_being_deleted_is_not_active_and_reserves_nothing() {
    // Arrange
    let rig = Rig::new().await;
    rig.exec(&format!(
        "UPDATE accounts SET state = 'deleting' WHERE id = '{}'",
        rig.account
    ))
    .await;

    // Act
    let result = rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::AccountNotActive)),
        "{result:?}"
    );
    assert_eq!(rig.reserved().await, 0);
    assert_eq!(rig.sessions().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn a_disabled_account_is_not_active() {
    // Arrange
    let rig = Rig::new().await;
    rig.exec(&format!(
        "UPDATE accounts SET state = 'disabled' WHERE id = '{}'",
        rig.account
    ))
    .await;

    // Act
    let result = rig.start(request(rig.root, "a.bin", 10 * MIB, "k1")).await;

    // Assert
    assert!(
        matches!(result, Err(UploadError::AccountNotActive)),
        "{result:?}"
    );
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}
