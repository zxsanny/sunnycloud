//! ST-03-1 (AZ-3758 AC-1, AC-59): account B uses account A's upload id, revision, node and blob, and A's
//! `client_key`, `staged_key` and `commit_id`. Every cross-account call is refused as not found, B's same keys
//! create B's own rows, A's stored state is identical afterwards, and B never receives A's receipt response.

use contracts::{
    BlobRef, CommitService, CommitServiceError, DownloadError, DownloadOptions, DownloadService,
    RevisionOrCurrent, UploadError, UploadService,
};
use uuid::Uuid;

use crate::batch_support::{StagedFile, batch, committed, entry};
use crate::commit_support::commit_request;
use crate::put_support::content;
use crate::security_support::actor;
use crate::start_support::{MIB, Rig, request, started};

#[tokio::test]
async fn transfer_calls_with_another_accounts_ids_and_keys_reach_nothing_of_it() {
    // Arrange
    let rig = Rig::new().await;
    let session = rig
        .upload_bytes("open.bin", "a-key", content(800, 3 * MIB as usize))
        .await;
    rig.put(&session, 0).await.expect("A's block");
    let staged_key = Uuid::new_v4();
    let staged_data = content(801, 5000);
    rig.stage(staged_key, &staged_data).await.expect("A staged");
    let files = rig.stage_files(rig.account, "a-batch", 2).await;
    let commit_id = Uuid::new_v4();
    let a_batch = rig
        .commit_batch(batch(
            commit_id,
            files.iter().map(|file| entry(file, rig.root)).collect(),
        ))
        .await
        .expect("A's batch");
    let a_file = rig
        .commit_bytes("a.txt", "a-file", content(802, 700), None)
        .await
        .expect("A's revision");
    let a_blob = rig.revision_row(a_file.revision_id).await.blob_id;
    let (b, b_root) = rig.other_account().await;
    let b_file = StagedFile {
        key: Uuid::new_v4(),
        name: "b.bin".to_owned(),
        data: content(803, 300),
    };
    rig.stage_for(b, &b_file).await;
    let before = rig.account_state(rig.account).await;
    let uploads: &dyn UploadService = rig.service.as_ref();
    let commits: &dyn CommitService = rig.service.as_ref();
    let downloads: &dyn DownloadService = rig.service.as_ref();

    // Act
    let get = uploads.get_upload(b, session.id).await;
    let put = rig
        .put_as(b, session.id, 1, session.hash(1), session.block(1))
        .await;
    let abort = uploads.abort_upload(b, session.id).await;
    let commit = commits
        .commit_upload(
            actor(b),
            commit_request(&session, b_root, "stolen.bin", None),
        )
        .await;
    let by_revision = downloads
        .open_revision_content(
            b,
            RevisionOrCurrent::Revision(a_file.revision_id),
            DownloadOptions {
                retained_ok: true,
                ..DownloadOptions::default()
            },
        )
        .await;
    let by_node = downloads
        .open_revision_content(
            b,
            RevisionOrCurrent::Current(a_file.node_id),
            DownloadOptions::default(),
        )
        .await;
    let blob = BlobRef {
        blob_id: a_blob,
        size: 700,
        wrapped_key: vec![0; 8],
        master_key_id: 1,
    };
    let by_blob = downloads
        .open_blob_content(b, blob, None, DownloadOptions::default())
        .await;
    let same_client_key = uploads
        .start_upload(b, request(b_root, "open.bin", 3 * MIB, "a-key"))
        .await;
    let same_staged_key = rig.stage_as(b, staged_key, &staged_data).await;
    let same_commit_id = rig
        .commit_batch_as(b, batch(commit_id, vec![entry(&b_file, b_root)]))
        .await;
    let after = rig.account_state(rig.account).await;

    // Assert
    for result in [get.map(drop), put.map(drop), abort] {
        assert!(
            matches!(result, Err(UploadError::UploadNotFound)),
            "{result:?}"
        );
    }
    assert!(
        matches!(commit, Err(CommitServiceError::UploadNotFound)),
        "{commit:?}"
    );
    for result in [by_revision, by_node, by_blob] {
        assert!(
            matches!(result, Err(DownloadError::NotFound)),
            "{:?}",
            result.map(|download| download.status)
        );
    }
    let b_session = started(same_client_key);
    assert_ne!(b_session.upload_id, session.id);
    assert!(same_staged_key.is_ok(), "{same_staged_key:?}");
    let b_batch = same_commit_id.expect("B's own batch");
    assert_ne!(b_batch, a_batch, "never A's stored response");
    committed(&b_batch.per_entry[0]);
    assert_eq!(rig.receipt_len(b, commit_id).await, Some(1));
    assert_eq!(after, before, "A's rows are identical");
    assert_eq!(
        rig.count(&format!(
            "SELECT count(*) FROM upload_sessions WHERE account_id = '{b}' \
             AND client_key IN ('a-key', '{staged_key}')"
        ))
        .await,
        2,
        "B's same keys are B's own rows"
    );
    rig.finish().await;
}
