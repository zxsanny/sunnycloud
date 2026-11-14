//! Downloads (AZ-3755 AC-2 to AC-4; IT-03-13): users never get a non-current revision, gone nodes, folders and
//! other accounts' content are not found, `Current(node)` follows the current revision, and only the seven
//! allowlisted sniffed types are inline, whatever the file name.

use contracts::{
    BlobRef, Disposition, Download, DownloadError, DownloadOptions, DownloadService,
    RevisionOrCurrent,
};
use uuid::Uuid;

use crate::batch_support::{StagedFile, batch, committed, entry};
use crate::download_support::read_all;
use crate::put_support::content;
use crate::start_support::Rig;

fn refusal(result: Result<Download, DownloadError>) -> DownloadError {
    match result {
        Ok(download) => panic!("expected a refusal, got {:?}", download.status),
        Err(error) => error,
    }
}

fn retained() -> DownloadOptions {
    DownloadOptions {
        retained_ok: true,
        ..DownloadOptions::default()
    }
}

#[tokio::test]
async fn open_revision_content_an_older_revision_is_served_only_with_retained_ok() {
    // Arrange
    let rig = Rig::new().await;
    let old_data = content(61, 3000);
    let old = rig
        .commit_bytes("doc.txt", "v1", old_data.clone(), None)
        .await
        .expect("v1");
    let new = rig
        .commit_bytes("doc.txt", "v2", content(62, 4000), Some(old.revision_id))
        .await
        .expect("v2");
    assert_eq!(new.node_id, old.node_id);
    let old_revision = RevisionOrCurrent::Revision(old.revision_id);

    // Act
    let as_user = rig.open(old_revision, DownloadOptions::default()).await;
    let as_admin = rig.open(old_revision, retained()).await.expect("retained");
    let current = rig
        .open(
            RevisionOrCurrent::Revision(new.revision_id),
            DownloadOptions::default(),
        )
        .await
        .expect("current revision");

    // Assert
    assert!(matches!(refusal(as_user), DownloadError::NotFound));
    assert!(read_all(as_admin).await == old_data, "old content");
    assert_eq!(current.total_size, 4000);
    rig.finish().await;
}

#[tokio::test]
async fn open_revision_content_folders_gone_files_and_foreign_ids_are_not_found() {
    // Arrange
    let rig = Rig::new().await;
    let folder = rig.folder("photos", false).await;
    let gone = rig
        .commit_bytes("gone.txt", "gone", content(63, 100), None)
        .await
        .expect("gone file");
    rig.exec(&format!(
        "UPDATE nodes SET deleted_at = 'epoch'::timestamptz WHERE id = '{}'",
        gone.node_id
    ))
    .await;
    let mine = rig
        .commit_bytes("mine.txt", "mine", content(64, 100), None)
        .await
        .expect("own file");
    let (other, _) = rig.other_account().await;
    let blob = rig.revision_row(mine.revision_id).await.blob_id;
    let blob_ref = BlobRef {
        blob_id: blob,
        size: 100,
        wrapped_key: vec![0; 8],
        master_key_id: 1,
    };
    let service: &dyn DownloadService = rig.service.as_ref();

    // Act
    let refusals = [
        rig.open_current(folder).await,
        rig.open_current(rig.root).await,
        rig.open_current(gone.node_id).await,
        rig.open(
            RevisionOrCurrent::Revision(gone.revision_id),
            DownloadOptions::default(),
        )
        .await,
        rig.open_as(
            other,
            RevisionOrCurrent::Current(mine.node_id),
            DownloadOptions::default(),
        )
        .await,
        rig.open_as(
            other,
            RevisionOrCurrent::Revision(mine.revision_id),
            retained(),
        )
        .await,
        service
            .open_blob_content(other, blob_ref, None, DownloadOptions::default())
            .await,
    ];

    // Assert
    for (index, result) in refusals.into_iter().enumerate() {
        let error = refusal(result);
        assert!(
            matches!(error, DownloadError::NotFound),
            "{index}: {error:?}"
        );
    }
    rig.finish().await;
}

#[tokio::test]
async fn open_revision_content_current_follows_the_new_revision_after_a_commit() {
    // Arrange
    let rig = Rig::new().await;
    let first = rig
        .commit_bytes("live.txt", "live1", content(65, 500), None)
        .await
        .expect("first");
    let before = read_all(rig.open_current(first.node_id).await.expect("first served")).await;
    let second_data = content(66, 700);
    rig.commit_bytes(
        "live.txt",
        "live2",
        second_data.clone(),
        Some(first.revision_id),
    )
    .await
    .expect("second");

    // Act
    let after = rig
        .open_current(first.node_id)
        .await
        .expect("second served");

    // Assert
    assert_eq!(before.len(), 500);
    assert!(read_all(after).await == second_data, "new content");
    rig.finish().await;
}

/// The heads of the 13 sniffed kinds of AC-4; every file is named like an image.
const HEADS: [(&[u8], Disposition); 13] = [
    (b"\xff\xd8\xff\xe0\x00\x10JFIF\x00", Disposition::Inline),
    (
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR",
        Disposition::Inline,
    ),
    (b"GIF89a\x01\x00\x01\x00\x80\x00\x00", Disposition::Inline),
    (
        b"RIFF\x1a\x00\x00\x00WEBPVP8L\x0d\x00\x00\x00",
        Disposition::Inline,
    ),
    (
        b"\x00\x00\x00\x1cftypavif\x00\x00\x00\x00avifmif1miaf",
        Disposition::Inline,
    ),
    (
        b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2",
        Disposition::Inline,
    ),
    (
        b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81\x01\x42\xf7\x81\x01\x42\x82\x84webm\x42\x87",
        Disposition::Inline,
    ),
    (
        b"<!DOCTYPE html><html><body><script>alert(1)</script>",
        Disposition::Attachment,
    ),
    (
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\">",
        Disposition::Attachment,
    ),
    (b"<?xml version=\"1.0\"?><note/>", Disposition::Attachment),
    (
        b"function f() { return document.cookie; }",
        Disposition::Attachment,
    ),
    (b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n", Disposition::Attachment),
    (
        b"\x13\x9a\x07\xee\x41\x00\x9f\x12\x88\x02\x7c\x55\x01\xd4\xe0\x3b",
        Disposition::Attachment,
    ),
];

#[tokio::test]
async fn open_revision_content_only_the_seven_allowlisted_sniffed_types_are_inline() {
    // Arrange
    let rig = Rig::new().await;
    let mut files = Vec::new();
    for (index, (head, _)) in HEADS.iter().enumerate() {
        let mut data = head.to_vec();
        data.extend(content(70 + index as u64, 2000));
        let file = StagedFile {
            key: Uuid::new_v4(),
            name: format!("photo-{index}.jpg"),
            data,
        };
        rig.stage_for(rig.account, &file).await;
        files.push(file);
    }
    let result = rig
        .commit_batch(batch(
            Uuid::new_v4(),
            files.iter().map(|file| entry(file, rig.root)).collect(),
        ))
        .await
        .expect("committed");

    for (index, outcome) in result.per_entry.iter().enumerate() {
        // Act
        let download = rig
            .open_current(committed(outcome).node_id)
            .await
            .expect("served");

        // Assert
        assert_eq!(download.disposition, HEADS[index].1, "file {index}");
    }
    rig.finish().await;
}
