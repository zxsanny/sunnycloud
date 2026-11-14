//! ST-03-2 (AZ-3758 AC-2; AC-7, AC-38): upload-protocol abuse on a session with acknowledged blocks. Every abuse
//! gets a typed error, 03 never calls `write_block` for it (so 04 is never asked to rewrite stored ciphertext),
//! and the encrypted file is byte-identical after each one.
//!
//! Observed and recorded, not decided here: `declared_size = 0` on the large path has no `UploadError` variant
//! and answers `Unexpected` (`upload_service.md` open item; 11 refuses it as `invalid_request`). A chunked body
//! without a length is 11's 411; at 03 every body comes with a declared length, and one that disagrees with the
//! bytes is `LengthMismatch`.

use contracts::UploadError;
use sc_ids::{BLOCK_SIZE, hash_block};

use crate::commit_support::commit_request;
use crate::put_support::{Upload, body, content};
use crate::start_support::{MIB, Rig, request};

const SIZE: usize = 9 * MIB as usize;

/// What abuse `name` answered, and whether the file and the write count stayed as they were.
struct Checked {
    name: &'static str,
    result: Result<(), UploadError>,
    unchanged: bool,
}

/// The file bytes and the `write_block` count once the good blocks are acknowledged.
struct Snapshot {
    file: Vec<u8>,
    writes: usize,
}

impl Snapshot {
    fn check(
        &self,
        rig: &Rig,
        upload: &Upload,
        name: &'static str,
        result: Result<(), UploadError>,
    ) -> Checked {
        let unchanged =
            rig.file_bytes(upload.blob) == self.file && rig.writer.writes() == self.writes;
        Checked {
            name,
            result,
            unchanged,
        }
    }
}

#[tokio::test]
async fn put_block_abuse_gets_typed_errors_and_never_rewrites_stored_ciphertext() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(SIZE, 810).await;
    for index in [0, 1, 4] {
        rig.put(&upload, index).await.expect("acked");
    }
    let snapshot = Snapshot {
        file: rig.file_bytes(upload.blob),
        writes: rig.writer.writes(),
    };
    let block = upload.block(2).to_vec();
    let length = block.len() as u64;
    let other = content(811, BLOCK_SIZE);
    let mut longer = block.clone();
    longer.push(0);
    let put = |index, hash, bytes: Vec<u8>, length| {
        let service = &rig.service;
        let (account, id) = (rig.account, upload.id);
        async move {
            let (stream, _) = body(&bytes, 4096);
            service
                .put_block(account, id, index, hash, stream, length)
                .await
                .map(drop)
        }
    };

    // Act
    let mut checked = Vec::new();
    let result = put(2, hash_block(&other), block.clone(), length).await;
    checked.push(snapshot.check(&rig, &upload, "wrong hash", result));
    let result = put(2, upload.hash(2), block[..MIB as usize].to_vec(), length).await;
    checked.push(snapshot.check(&rig, &upload, "short body", result));
    let result = put(2, upload.hash(2), longer, length).await;
    checked.push(snapshot.check(&rig, &upload, "long body", result));
    let result = put(2, upload.hash(2), block.clone(), 0).await;
    checked.push(snapshot.check(&rig, &upload, "chunked, no length", result));
    let result = put(0, hash_block(&other), other.clone(), length).await;
    checked.push(snapshot.check(&rig, &upload, "other bytes for an acked index", result));
    let result = put(99, upload.hash(2), block.clone(), length).await;
    checked.push(snapshot.check(&rig, &upload, "index out of range", result));
    let zero = rig.start(request(rig.root, "zero.bin", 0, "zero")).await;
    let huge = rig
        .start(request(rig.root, "huge.bin", i64::MAX, "huge"))
        .await;

    // Assert
    let expected = [
        "HashMismatch",
        "LengthMismatch",
        "LengthMismatch",
        "LengthMismatch",
        "BlockConflict",
        "IndexOutOfRange",
    ];
    for (check, code) in checked.iter().zip(expected) {
        let got = check
            .result
            .as_ref()
            .err()
            .map(|error| format!("{error:?}"));
        assert!(
            got.as_deref().is_some_and(|got| got.starts_with(code)),
            "{}: {got:?}",
            check.name
        );
        assert!(
            check.unchanged,
            "{}: the file or the write count changed",
            check.name
        );
    }
    assert!(matches!(zero, Err(UploadError::Unexpected(_))), "{zero:?}");
    assert!(matches!(huge, Err(UploadError::SizeTooLarge)), "{huge:?}");
    assert_eq!(rig.sessions().await, 1, "neither start left a row");
    rig.finish().await;
}

#[tokio::test]
async fn put_block_after_commit_is_not_found_and_leaves_the_file_unchanged() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig
        .upload_bytes("done.bin", "done", content(812, 3 * MIB as usize))
        .await;
    rig.send_all(&upload).await;
    rig.commit(commit_request(&upload, rig.root, "done.bin", None))
        .await
        .expect("committed");
    let file = rig.file_bytes(upload.blob);
    let writes = rig.writer.writes();

    // Act
    let replay = rig.put(&upload, 0).await;
    let other = content(813, BLOCK_SIZE);
    let (rewrite, _) = rig
        .put_raw(upload.id, 0, hash_block(&other), &other, other.len() as u64)
        .await;

    // Assert
    for result in [replay, rewrite] {
        assert!(
            matches!(result, Err(UploadError::UploadNotFound)),
            "{result:?}"
        );
    }
    assert_eq!(rig.file_bytes(upload.blob), file);
    assert_eq!(rig.writer.writes(), writes);
    rig.finish().await;
}
