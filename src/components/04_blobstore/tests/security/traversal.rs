//! ST-04-4 (AC-7): blob paths come only from server-generated ids. Hostile id texts fail to parse (typed); ids
//! a lenient parser or raw bytes can still forge reach `open_for_write`, `open`, `read_head` and `discard` and
//! find nothing. A path recorder on the blob root, its parent and a canary `etc/passwd` beside the root shows
//! that every touched path lies under the root.

use std::fs;

use contracts::{BlobError, BlobReader, BlobWriter};
use sunnycloud_blobstore::{FormatError, parse_blob_id};
use uuid::Uuid;

use crate::kit::{read_all, store};
use crate::support::{MIB, Rig};
use crate::watch::PathRecorder;

const V4: &str = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b";
const CANARY: &[u8] = b"root:x:0:0:canary\n";

fn hostile_texts() -> Vec<String> {
    vec![
        "../../etc/passwd".to_owned(),
        "00/../..".to_owned(),
        "not-a-uuid".to_owned(),
        V4.to_uppercase(),
        format!("{{{V4}}}"),
    ]
}

/// Ids the type can still hold: a lenient parse of the uppercase and braced spellings, and raw bytes that spell
/// traversal text.
fn forged_ids() -> Vec<Uuid> {
    vec![
        Uuid::try_parse(&V4.to_uppercase()).expect("lenient parse of uppercase"),
        Uuid::try_parse(&format!("{{{V4}}}")).expect("lenient parse of braces"),
        Uuid::from_bytes(*b"../../etc/passwd"),
        Uuid::from_bytes(*b"00/../..00/../.."),
    ]
}

fn is_typed_refusal(result: &Result<(), BlobError>) -> bool {
    matches!(
        result,
        Err(BlobError::BlobMissing) | Err(BlobError::KeyUnavailable)
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn hostile_ids_fail_typed_and_every_touched_path_starts_with_the_blob_root() {
    // Arrange
    let rig = Rig::new().await;
    let control = store(&rig, rig.account, &vec![5; MIB]).await;
    let outside = rig._scratch.0.clone();
    fs::create_dir_all(outside.join("etc")).expect("canary dir");
    let canary = outside.join("etc").join("passwd");
    fs::write(&canary, CANARY).expect("canary");
    let control_path = rig.path(control.blob_id);
    let mut recorder = PathRecorder::new();
    for dir in [
        outside.clone(),
        outside.join("etc"),
        rig.volume.clone(),
        rig.volume.join("blobs"),
        control_path.parent().expect("prefix dir").to_path_buf(),
    ] {
        recorder.watch(&dir);
    }

    // Act
    let parsed: Vec<_> = hostile_texts()
        .iter()
        .map(|text| parse_blob_id(text))
        .collect();
    let mut outcomes = Vec::new();
    for id in forged_ids() {
        let reopened = rig
            .writer
            .open_for_write(
                id,
                &control.wrapped_key,
                control.master_key_id,
                control.size,
            )
            .await
            .map(drop);
        let opened = read_all(&rig.reader, id, &control).await;
        let head = rig
            .reader
            .read_head(
                id,
                &control.wrapped_key,
                control.master_key_id,
                control.size,
                4096,
            )
            .await
            .map(drop);
        let discarded = rig.writer.discard(id).await;
        outcomes.push((id, reopened, opened, head, discarded));
    }
    let control_read = read_all(&rig.reader, control.blob_id, &control).await;
    let touched = recorder.paths();

    // Assert
    assert!(
        parsed
            .iter()
            .all(|result| matches!(result, Err(FormatError::InvalidBlobId))),
        "{parsed:?}"
    );
    for (id, reopened, opened, head, discarded) in &outcomes {
        assert!(
            is_typed_refusal(reopened),
            "{id} open_for_write: {reopened:?}"
        );
        assert!(
            opened.bytes.is_empty()
                && matches!(
                    opened.open,
                    Some(BlobError::BlobMissing | BlobError::KeyUnavailable)
                ),
            "{id} open: {opened:?}"
        );
        assert!(is_typed_refusal(head), "{id} read_head: {head:?}");
        assert!(
            discarded.is_ok(),
            "{id} discard of a missing id: {discarded:?}"
        );
    }
    assert_eq!(control_read.bytes.len(), MIB, "the control blob reads");
    assert!(
        touched.contains(&control_path),
        "the recorder saw the control read: {touched:?}"
    );
    let escaped: Vec<_> = touched
        .iter()
        .filter(|path| !path.starts_with(&rig.volume))
        .collect();
    assert!(escaped.is_empty(), "paths outside the root: {escaped:?}");
    assert_eq!(fs::read(&canary).expect("canary"), CANARY);
    rig.finish().await;
}
