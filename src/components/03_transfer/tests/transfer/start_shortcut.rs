//! AZ-3734 AC-6 and AC-6b (IT-03-1, `upload_service.md` 1.2.0 § `NothingToSend` rule): a start answers
//! `NothingToSend` with 0 bytes and nothing reserved when the content is already the current revision, or when its
//! key committed within `upload_session_ttl`; every other case of the rule starts a session.

use contracts::{StartUpload, StartUploadOutcome};
use uuid::Uuid;

use crate::start_support::{
    MIB, Rig, close, committed_session, file, hash, node, request, revision, started,
    ticks_past_ttl,
};

fn with_hash(parent: Uuid, name: &str, content: u8) -> StartUpload {
    StartUpload {
        content_hash: Some(hash(content)),
        ..request(parent, name, MIB, "k1")
    }
}

fn nothing_to_send(revision: Uuid) -> StartUploadOutcome {
    StartUploadOutcome::NothingToSend { revision }
}

#[tokio::test]
async fn the_same_hash_at_the_name_has_nothing_to_send() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (_, current) = file(&mut owner, rig.account, rig.root, "photo.jpg", &hash(1)).await;
    close(owner).await;

    // Act
    let outcome = rig.start(with_hash(rig.root, "photo.jpg", 1)).await;

    // Assert
    assert_eq!(outcome.expect("start"), nothing_to_send(current));
    assert_eq!(rig.reserved().await, 0);
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.files(), 0);
    rig.finish().await;
}

#[tokio::test]
async fn the_name_is_matched_by_its_key_whatever_the_case() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (_, current) = file(&mut owner, rig.account, rig.root, "Photo.JPG", &hash(1)).await;
    close(owner).await;

    // Act
    let outcome = rig.start(with_hash(rig.root, "photo.jpg", 1)).await;

    // Assert
    assert_eq!(outcome.expect("start"), nothing_to_send(current));
    rig.finish().await;
}

#[tokio::test]
async fn another_hash_at_the_name_starts_a_session() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    file(&mut owner, rig.account, rig.root, "photo.jpg", &hash(1)).await;
    close(owner).await;

    // Act
    let outcome = rig.start(with_hash(rig.root, "photo.jpg", 2)).await;

    // Assert
    started(outcome);
    assert_eq!(rig.reserved().await, MIB);
    rig.finish().await;
}

#[tokio::test]
async fn without_a_hash_an_existing_file_starts_a_session() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    file(&mut owner, rig.account, rig.root, "photo.jpg", &hash(1)).await;
    close(owner).await;

    // Act
    let outcome = rig.start(request(rig.root, "photo.jpg", MIB, "k1")).await;

    // Assert
    started(outcome);
    rig.finish().await;
}

#[tokio::test]
async fn a_tombstoned_file_a_folder_or_no_node_at_the_name_starts_a_session() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (gone, _) = file(&mut owner, rig.account, rig.root, "gone.jpg", &hash(1)).await;
    node(&mut owner, rig.account, Some(rig.root), "folder", "album").await;
    close(owner).await;
    rig.exec(&format!(
        "UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = '{gone}'"
    ))
    .await;

    // Act
    let outcomes = [
        rig.start(StartUpload {
            client_key: "tomb".to_owned(),
            ..with_hash(rig.root, "gone.jpg", 1)
        })
        .await,
        rig.start(StartUpload {
            client_key: "folder".to_owned(),
            ..with_hash(rig.root, "album", 1)
        })
        .await,
        rig.start(StartUpload {
            client_key: "none".to_owned(),
            ..with_hash(rig.root, "new.jpg", 1)
        })
        .await,
    ];

    // Assert
    for outcome in outcomes {
        started(outcome);
    }
    assert_eq!(rig.sessions().await, 3);
    rig.finish().await;
}

#[tokio::test]
async fn a_base_revision_matches_its_own_node_not_the_name() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (_, current) = file(&mut owner, rig.account, rig.root, "moved.jpg", &hash(1)).await;
    file(&mut owner, rig.account, rig.root, "photo.jpg", &hash(1)).await;
    close(owner).await;
    let request = StartUpload {
        base_revision: Some(current),
        ..with_hash(rig.root, "photo.jpg", 1)
    };

    // Act
    let outcome = rig.start(request).await;

    // Assert
    assert_eq!(outcome.expect("start"), nothing_to_send(current));
    rig.finish().await;
}

#[tokio::test]
async fn a_base_whose_node_holds_another_hash_starts_a_session() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (node_id, current) = file(&mut owner, rig.account, rig.root, "a.jpg", &hash(1)).await;
    let older = revision(&mut owner, rig.account, node_id, &hash(2)).await;
    close(owner).await;
    let request = StartUpload {
        base_revision: Some(older),
        ..with_hash(rig.root, "a.jpg", 2)
    };

    // Act
    let outcome = rig.start(request).await;

    // Assert
    assert_ne!(older, current);
    started(outcome);
    rig.finish().await;
}

#[tokio::test]
async fn an_unknown_base_revision_starts_a_session() {
    // Arrange
    let rig = Rig::new().await;
    let request = StartUpload {
        base_revision: Some(Uuid::new_v4()),
        ..with_hash(rig.root, "a.jpg", 1)
    };

    // Act
    let outcome = rig.start(request).await;

    // Assert
    started(outcome);
    rig.finish().await;
}

#[tokio::test]
async fn a_committed_key_answers_its_revision_whatever_the_name_holds_now() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (moved, committed) =
        file(&mut owner, rig.account, rig.root, "elsewhere.jpg", &hash(1)).await;
    committed_session(&mut owner, rig.account, "k1", committed).await;
    file(&mut owner, rig.account, rig.root, "photo.jpg", &hash(9)).await;
    close(owner).await;
    rig.exec(&format!(
        "UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = '{moved}'"
    ))
    .await;

    // Act
    let outcome = rig.start(request(rig.root, "photo.jpg", MIB, "k1")).await;

    // Assert
    assert_eq!(outcome.expect("start"), nothing_to_send(committed));
    assert_eq!(rig.reserved().await, 0);
    assert_eq!(rig.sessions().await, 1);
    assert_eq!(rig.uploading_blobs().await, 0);
    rig.finish().await;
}

#[tokio::test]
async fn after_the_ttl_a_committed_key_starts_a_new_session() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    let (_, committed) = file(&mut owner, rig.account, rig.root, "photo.jpg", &hash(1)).await;
    committed_session(&mut owner, rig.account, "k1", committed).await;
    close(owner).await;
    ticks_past_ttl(&rig.clock);

    // Act
    let session = started(
        rig.start(request(rig.root, "photo.jpg", 3 * MIB, "k1"))
            .await,
    );

    // Assert
    assert_eq!(session.need_blocks, vec![0, 1]);
    assert_eq!(rig.reserved().await, 3 * MIB);
    assert_eq!(rig.sessions().await, 1);
    assert_eq!(
        rig.count(&format!(
            "SELECT count(*) FROM upload_sessions WHERE account_id = '{}' AND committed_revision_id IS NULL",
            rig.account
        ))
        .await,
        1
    );
    rig.finish().await;
}

#[tokio::test]
async fn committed_rows_do_not_count_toward_the_session_cap() {
    // Arrange
    let rig = Rig::new().await;
    let mut owner = rig.owner().await;
    for index in 0..8 {
        let name = format!("done{index}");
        let (_, committed) = file(&mut owner, rig.account, rig.root, &name, &hash(1)).await;
        committed_session(&mut owner, rig.account, &format!("c{index}"), committed).await;
    }
    close(owner).await;

    // Act
    let outcome = rig.start(request(rig.root, "new.jpg", MIB, "k1")).await;

    // Assert
    started(outcome);
    assert_eq!(rig.sessions().await, 9);
    rig.finish().await;
}
