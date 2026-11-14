//! `commit_upload` target choice and outcomes (AZ-3738 AC-4, IT-03-7) and a session removed between validation
//! and `publish` (AC-5, IT-03-9 case a). `commit_service.md` fixes the fields: a stale base answers the committed
//! file with the older content in `conflict_copy`; a taken name answers the new file twice, under a
//! conflict-copy name; neither is an error.

use std::sync::Arc;

use contracts::CommitServiceError;
use uuid::Uuid;

use crate::commit_support::{DEVICE, commit_request};
use crate::put_support::content;
use crate::start_support::{MIB, Rig, close, file, hash};

const NAME: &str = "notes.txt";

/// The file `notes.txt` committed once; returns its node and revision.
async fn first_version(rig: &Rig) -> (Uuid, Uuid) {
    let committed = rig
        .commit_bytes(NAME, "v1", content(91, MIB as usize), None)
        .await
        .expect("first version");
    (committed.node_id, committed.revision_id)
}

#[tokio::test]
async fn commit_upload_on_the_current_base_adds_a_revision_to_the_same_node() {
    // Arrange
    let rig = Rig::new().await;
    let (node, first) = first_version(&rig).await;

    // Act
    let committed = rig
        .commit_bytes(NAME, "v2", content(92, MIB as usize), Some(first))
        .await
        .expect("second version");

    // Assert
    assert_eq!(committed.node_id, node);
    assert_ne!(committed.revision_id, first);
    assert_eq!(committed.conflict_copy, None);
    let (name, current) = rig.node_row(node).await;
    assert_eq!(
        (name.as_str(), current),
        (NAME, Some(committed.revision_id))
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_on_a_stale_base_keeps_the_older_content_in_a_conflict_copy() {
    // Arrange
    let rig = Rig::new().await;
    let (node, first) = first_version(&rig).await;
    let second = rig
        .commit_bytes(NAME, "v2", content(92, MIB as usize), Some(first))
        .await
        .expect("second version")
        .revision_id;

    // Act
    let committed = rig
        .commit_bytes(NAME, "v3", content(93, MIB as usize), Some(first))
        .await;

    // Assert
    let committed = committed.expect("a stale base commits");
    assert_eq!(committed.node_id, node);
    let copy = committed.conflict_copy.expect("a conflict copy");
    assert_ne!(copy, node);
    let (_, current) = rig.node_row(node).await;
    assert_eq!(current, Some(committed.revision_id), "new content current");
    let (copy_name, copy_current) = rig.node_row(copy).await;
    assert_eq!(
        copy_current,
        Some(second),
        "the copy holds the older content"
    );
    assert!(
        copy_name.starts_with("notes (conflict copy ") && copy_name.contains(DEVICE),
        "{copy_name}"
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_of_a_new_file_on_a_taken_name_gets_a_conflict_copy_name() {
    // Arrange
    let rig = Rig::new().await;
    let (node, _) = first_version(&rig).await;

    // Act
    let committed = rig
        .commit_bytes(NAME, "other", content(94, MIB as usize), None)
        .await;

    // Assert
    let committed = committed.expect("a taken name commits");
    assert_ne!(committed.node_id, node);
    assert_eq!(committed.conflict_copy, Some(committed.node_id));
    let (name, current) = rig.node_row(committed.node_id).await;
    assert_eq!(current, Some(committed.revision_id));
    assert!(
        name.starts_with("notes (conflict copy ") && name.ends_with(&format!("{DEVICE}).txt")),
        "{name}"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commit_upload_after_an_abort_between_validation_and_publish_is_not_found() {
    // Arrange
    let rig = Rig::new().await;
    let size = 4 * MIB;
    let upload = rig.upload(size as usize, 95).await;
    rig.send_all(&upload).await;
    // Another open session keeps a reservation, so a second release would show (the ledger clamps at zero).
    let bystander = 6 * MIB;
    rig.upload(bystander as usize, 96).await;
    let (reached, resume) = rig.commit.pause_next();
    let service = Arc::clone(&rig.service);
    let actor = rig.actor();
    let request = commit_request(&upload, rig.root, "aborted.bin", None);
    let commit = tokio::spawn(async move { service.commit_upload(actor, request).await });
    reached
        .await
        .expect("validation passed and publish is next");
    rig.service
        .abort_upload(rig.account, upload.id)
        .await
        .expect("abort removes the session");
    let reserved_after_abort = rig.reserved().await;

    // Act
    resume.send(()).expect("the commit waits");
    let result = commit.await.expect("commit task");

    // Assert
    assert!(
        matches!(result, Err(CommitServiceError::UploadNotFound)),
        "{result:?}"
    );
    assert_eq!(rig.revisions().await, 0, "no revision");
    assert_eq!(rig.used().await, 0, "usage unchanged");
    assert_eq!(reserved_after_abort, bystander, "released by the abort");
    assert_eq!(
        rig.reserved().await,
        bystander,
        "not released again by the commit"
    );
    assert_eq!(rig.sessions().await, 1, "only the bystander's session");
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commit_upload_whose_session_closed_before_the_effect_rolls_back_its_entry() {
    // Arrange: between validation and publish another call closes the session into its committed record (and
    // settles its reservation), leaving the blob row; only `BlobCommitEffect`'s lock can tell.
    let rig = Rig::new().await;
    let size = 4 * MIB;
    let upload = rig.upload(size as usize, 97).await;
    rig.send_all(&upload).await;
    let bystander = 6 * MIB;
    rig.upload(bystander as usize, 98).await;
    let mut conn = rig.owner().await;
    let (_, other) = file(&mut conn, rig.account, rig.root, "other.bin", &hash(7)).await;
    close(conn).await;
    let revisions_before = rig.revisions().await;
    let (reached, resume) = rig.commit.pause_next();
    let service = Arc::clone(&rig.service);
    let actor = rig.actor();
    let request = commit_request(&upload, rig.root, "closed.bin", None);
    let commit = tokio::spawn(async move { service.commit_upload(actor, request).await });
    reached
        .await
        .expect("validation passed and publish is next");
    rig.exec(&format!(
        "UPDATE upload_sessions SET committed_revision_id = '{other}', reserved_bytes = 0 WHERE id = '{}'",
        upload.id
    ))
    .await;
    rig.exec(&format!(
        "UPDATE quota SET reserved_bytes = reserved_bytes - {size} WHERE account_id = '{}'",
        rig.account
    ))
    .await;

    // Act
    resume.send(()).expect("the commit waits");
    let result = commit.await.expect("commit task");

    // Assert
    assert!(
        matches!(result, Err(CommitServiceError::UploadNotFound)),
        "{result:?}"
    );
    assert_eq!(rig.revisions().await, revisions_before, "entry rolled back");
    assert_eq!(rig.used().await, 0, "its usage delta rolled back");
    assert_eq!(rig.reserved().await, bystander, "its release rolled back");
    assert_eq!(
        rig.blob_state(upload.blob).await.as_deref(),
        Some("uploading")
    );
    rig.finish().await;
}

#[tokio::test]
async fn commit_upload_of_the_current_content_changes_nothing_and_drops_the_unused_blob() {
    // Arrange
    let rig = Rig::new().await;
    let data = content(99, 3 * MIB as usize);
    let first = rig
        .commit_bytes(NAME, "same-1", data.clone(), None)
        .await
        .expect("first version");
    let used_before = rig.used().await;
    let upload = rig.upload_bytes(NAME, "same-2", data).await;
    rig.send_all(&upload).await;
    let files_before = rig.files();

    // Act
    let committed = rig
        .commit(commit_request(
            &upload,
            rig.root,
            NAME,
            Some(first.revision_id),
        ))
        .await
        .expect("no change");

    // Assert
    assert_eq!(
        (
            committed.node_id,
            committed.revision_id,
            committed.conflict_copy
        ),
        (first.node_id, first.revision_id, None)
    );
    assert_eq!(rig.session_record(upload.id).await, None, "session gone");
    assert_eq!(
        rig.blob_state(upload.blob).await,
        None,
        "unused blob row gone"
    );
    assert_eq!(rig.files(), files_before - 1, "unused file discarded");
    assert_eq!(rig.used().await, used_before);
    assert_eq!(rig.reserved().await, 0);
    rig.finish().await;
}
