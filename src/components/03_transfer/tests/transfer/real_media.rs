//! AZ-3760 AC-2 (IT-03-9) and AC-3 (IT-03-8, AC-40): the real 08 `MediaEnqueueEffect` in 03's effect slot runs in
//! the commit transaction, after 03's own effects. A job row exists only for an entry that committed: none for a
//! JPEG whose session went before `publish`, none for an entry refused by its name, and none added by a replay.

use std::sync::Arc;

use contracts::{BatchEntryResult, CommitServiceError, EntryError};
use uuid::Uuid;

use crate::batch_support::{StagedFile, batch, entry};
use crate::commit_support::{commit_request, file_hash};
use crate::real_support::{Real, jpeg};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commit_upload_enqueues_a_job_for_the_committed_jpeg_and_none_for_the_one_whose_session_went()
 {
    // Arrange
    let real = Real::new().await;
    let rig = &real.rig;
    let (kept, gone) = (jpeg("kept"), jpeg("gone"));
    let first = rig.upload_bytes("kept.jpg", "kept", kept.clone()).await;
    rig.send_all(&first).await;
    let second = rig.upload_bytes("gone.jpg", "gone", gone.clone()).await;
    rig.send_all(&second).await;
    let (reached, resume) = rig.commit.pause_next();
    let service = Arc::clone(&rig.service);
    let request = commit_request(&second, rig.root, "gone.jpg", None);
    let actor = rig.actor();
    let held = tokio::spawn(async move { service.commit_upload(actor, request).await });
    reached
        .await
        .expect("validation passed and publish is next");
    rig.service
        .abort_upload(rig.account, second.id)
        .await
        .expect("abort removes the session");

    // Act
    let committed = rig
        .commit(commit_request(&first, rig.root, "kept.jpg", None))
        .await;
    resume.send(()).expect("the held commit waits");
    let refused = held.await.expect("commit task");

    // Assert
    let committed = committed.expect("the first commit");
    assert!(
        matches!(refused, Err(CommitServiceError::UploadNotFound)),
        "{refused:?}"
    );
    assert_eq!(
        rig.revision_row(committed.revision_id)
            .await
            .media_type
            .as_deref(),
        Some("image/jpeg")
    );
    assert_eq!(
        real.jobs_of(&file_hash(&kept)).await,
        1,
        "a job for the committed JPEG"
    );
    assert_eq!(
        real.jobs_of(&file_hash(&gone)).await,
        0,
        "none for the one whose session went"
    );
    assert_eq!(rig.revisions().await, 1);
    real.finish().await;
}

#[tokio::test]
async fn commit_batch_enqueues_jobs_for_the_committed_images_only_and_a_replay_adds_none() {
    // Arrange
    let real = Real::new().await;
    let rig = &real.rig;
    let mut files = Vec::new();
    for name in ["one", "bad", "three"] {
        let file = StagedFile {
            key: Uuid::new_v4(),
            name: format!("{name}.jpg"),
            data: jpeg(name),
        };
        rig.stage(file.key, &file.data).await.expect("staged");
        files.push(file);
    }
    let mut entries: Vec<_> = files.iter().map(|file| entry(file, rig.root)).collect();
    entries[1].name = "bad/name.jpg".to_owned();
    let request = batch(Uuid::new_v4(), entries);

    // Act
    let result = rig
        .commit_batch(request.clone())
        .await
        .expect("batch committed");
    let (jobs, seq) = (real.jobs().await, real.seq().await);
    let replay = rig.commit_batch(request).await.expect("replayed");

    // Assert
    assert!(
        matches!(result.per_entry[0], BatchEntryResult::Committed(_)),
        "{result:?}"
    );
    assert_eq!(
        result.per_entry[1],
        BatchEntryResult::Failed(EntryError::InvalidName)
    );
    assert!(
        matches!(result.per_entry[2], BatchEntryResult::Committed(_)),
        "{result:?}"
    );
    assert_eq!(real.jobs_of(&file_hash(&files[0].data)).await, 1);
    assert_eq!(real.jobs_of(&file_hash(&files[1].data)).await, 0);
    assert_eq!(real.jobs_of(&file_hash(&files[2].data)).await, 1);
    assert_eq!(jobs, 2, "job rows for the 2 committed images only");
    assert_eq!(replay, result, "the stored response");
    assert_eq!(real.jobs().await, jobs, "the replay adds no job row");
    assert_eq!(real.seq().await, seq, "the replay consumed no seq");
    real.finish().await;
}
