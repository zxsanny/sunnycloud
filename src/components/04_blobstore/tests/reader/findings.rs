//! AC-5: a corrupt blob is marked and audited once. AC-6: a missing file and an unknown key id are typed. A stream
//! whose producer is lost ends with an error, never with a normal end.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::FileExt;

use contracts::{BlobError, StreamError};
use uuid::Uuid;

use crate::support::{MIB, Rig, consume, data};

const CHUNK: usize = 65_536;

fn flip_in_chunk(rig: &Rig, blob_id: Uuid, chunk: usize) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(rig.path(blob_id))
        .expect("blob file");
    let at = 64 + (chunk * 65_552 + 20) as u64;
    let mut byte = [0];
    file.read_exact_at(&mut byte, at).expect("read byte");
    byte[0] ^= 0x01;
    file.write_all_at(&byte, at).expect("write byte");
}

async fn events(rig: &Rig, blob_id: Uuid, reason: &str) -> i64 {
    rig.db
        .admin_count(&format!(
            "SELECT count(*) FROM audit_log WHERE event = 'scrub_mismatch' AND subject_type = 'blob' \
             AND subject_id = '{blob_id}' AND detail->>'reason' = '{reason}' AND detail->>'source' = 'reader'"
        ))
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_corrupt_read_marks_the_blob_and_every_later_one_adds_no_event() {
    // Arrange
    let rig = Rig::new().await;
    let at_open = rig.store(&data(0x51, MIB)).await;
    let in_stream = rig.store(&data(0x52, MIB)).await;
    flip_in_chunk(&rig, at_open.blob_id, 0);
    flip_in_chunk(&rig, in_stream.blob_id, 5);

    // Act
    let first = rig.open(&at_open, None).await;
    let second = rig.open(&at_open, None).await;
    let racing = futures_join_all([
        rig.open(&at_open, None),
        rig.open(&at_open, None),
        rig.open(&at_open, None),
    ])
    .await;
    let streamed = [
        consume(rig.open(&in_stream, None).await.expect("open")).await,
        consume(rig.open(&in_stream, None).await.expect("open")).await,
    ];

    // Assert
    assert!(matches!(first, Err(BlobError::Corrupt)));
    assert!(matches!(second, Err(BlobError::Corrupt)));
    assert!(
        racing
            .iter()
            .all(|result| matches!(result, Err(BlobError::Corrupt)))
    );
    assert!(
        streamed
            .iter()
            .all(|read| matches!(read.error, Some(StreamError::Corrupt)))
    );
    assert!(rig.unavailable(at_open.blob_id).await);
    assert!(rig.unavailable(in_stream.blob_id).await);
    assert_eq!(events(&rig, at_open.blob_id, "corrupt").await, 1);
    assert_eq!(events(&rig, in_stream.blob_id, "corrupt").await, 1);
    assert_eq!(rig.findings(at_open.blob_id).await, 1);
    rig.finish().await;
}

async fn futures_join_all<F: std::future::Future>(futures: [F; 3]) -> Vec<F::Output> {
    let [a, b, c] = futures;
    let (a, b, c) = tokio::join!(a, b, c);
    vec![a, b, c]
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_file_is_blob_missing_for_open_and_read_head_and_is_recorded_once() {
    // Arrange
    let rig = Rig::new().await;
    let blob = rig.store(&data(0x61, MIB)).await;
    fs::remove_file(rig.path(blob.blob_id)).expect("delete the file");

    // Act
    let opened = rig.open(&blob, None).await;
    let head = rig.head(&blob, 4096).await;

    // Assert
    assert!(
        matches!(opened, Err(BlobError::BlobMissing)),
        "{:?}",
        opened.err()
    );
    assert!(matches!(head, Err(BlobError::BlobMissing)), "{head:?}");
    assert!(rig.unavailable(blob.blob_id).await);
    assert_eq!(events(&rig, blob.blob_id, "missing").await, 1);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_file_of_a_blob_that_is_no_longer_committed_is_not_a_finding() {
    // Arrange
    let rig = Rig::new().await;
    let blob = rig.store(&data(0x62, CHUNK)).await;
    rig.db
        .admin_exec(&format!(
            "UPDATE blobs SET state = 'gc_pending' WHERE id = '{}'",
            blob.blob_id
        ))
        .await;
    fs::remove_file(rig.path(blob.blob_id)).expect("delete the file");

    // Act
    let opened = rig.open(&blob, None).await;

    // Assert
    assert!(matches!(opened, Err(BlobError::BlobMissing)));
    assert!(!rig.unavailable(blob.blob_id).await);
    assert_eq!(rig.findings(blob.blob_id).await, 0);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_master_key_id_the_ring_lacks_is_key_unavailable_for_open_and_read_head() {
    // Arrange
    let rig = Rig::new().await;
    let blob = rig.store(&data(0x71, MIB)).await;

    // Act
    let opened = contracts::BlobReader::open(
        &rig.reader,
        blob.blob_id,
        &blob.wrapped_key,
        99,
        blob.size,
        None,
    )
    .await;
    let head = contracts::BlobReader::read_head(
        &rig.reader,
        blob.blob_id,
        &blob.wrapped_key,
        99,
        blob.size,
        4096,
    )
    .await;

    // Assert
    assert!(
        matches!(opened, Err(BlobError::KeyUnavailable)),
        "{:?}",
        opened.err()
    );
    assert!(matches!(head, Err(BlobError::KeyUnavailable)), "{head:?}");
    assert!(!rig.unavailable(blob.blob_id).await);
    assert_eq!(rig.findings(blob.blob_id).await, 0);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_whose_producer_is_lost_ends_with_an_error_and_not_with_a_short_body() {
    // Arrange
    let rig = Rig::new().await;
    let size = 3 * MIB;
    let source = data(0x81, size);
    let blob = rig.store(&source).await;
    let stream = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .build()
                    .expect("second runtime");
                let stream = runtime.block_on(rig.open(&blob, None)).expect("open");
                drop(runtime);
                stream
            })
            .join()
            .expect("thread finishes")
    });

    // Act
    let consumed = consume(stream).await;

    // Assert
    assert!(matches!(consumed.error, Some(StreamError::Source(_))));
    assert!(consumed.bytes.len() < size);
    assert!(consumed.bytes == source[..consumed.bytes.len()]);
    assert_eq!(consumed.after_error, 0);
    rig.finish().await;
}
