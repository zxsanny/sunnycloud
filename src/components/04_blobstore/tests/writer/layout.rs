//! AC-2: out-of-order writes give the specified layout and no plaintext reaches the file.

use std::fs;
use std::sync::Arc;

use contracts::BlobWriter;
use sunnycloud_blobstore::{FORMAT_VERSION, HEADER_LEN, Header};
use uuid::Uuid;

use crate::support::{MIB, Rig, created, data, decrypt, marker, seed_of, write_shuffled};

const SIZES: [usize; 7] = [0, 1, 65_535, 65_536, 65_537, 2 * MIB, 5 * MIB + 7];

fn hits(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn shuffled_writes_from_four_tasks_give_the_layout_and_no_marker_for_every_size() {
    // Arrange
    let rig = Rig::new().await;
    for size in SIZES {
        let (handle, source) = created(&rig, size, true).await;
        let marker = marker(seed_of(size));
        let placed = (0..size).step_by(MIB).filter(|at| at + 64 <= size).count();
        assert_eq!(
            hits(&source, &marker),
            placed,
            "marker every 1 MiB in {size}"
        );

        // Act
        write_shuffled(&rig.writer, &handle, &source).await;

        // Assert
        let file = fs::read(rig.path(handle.plan.blob_id)).expect("file");
        let chunks = size.div_ceil(65_536).max(1);
        assert_eq!(file.len(), 64 + chunks * 16 + size, "length for {size}");
        assert_eq!(&file[..8], b"SUNNYBLB");
        assert_eq!(file[8..12], FORMAT_VERSION.to_be_bytes());
        Header::decode(&file[..HEADER_LEN]).expect("header decodes");
        assert_eq!(hits(&file, &marker), 0, "marker for {size}");
        assert_eq!(
            decrypt(&rig, &handle.plan, size),
            *source,
            "round trip {size}"
        );
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn identical_plaintext_gives_different_ciphertext_and_wrapped_key() {
    // Arrange
    let rig = Rig::new().await;
    let source = Arc::new(data(9, 3 * MIB));
    let mut files = Vec::new();
    let mut plans = Vec::new();
    for _ in 0..2 {
        let plan = rig
            .writer
            .plan(Uuid::new_v4(), source.len() as i64)
            .expect("plan");
        let handle = rig
            .writer
            .create_file(plan, source.len() as i64, true)
            .await
            .expect("create");

        // Act
        write_shuffled(&rig.writer, &handle, &source).await;

        // Assert
        files.push(fs::read(rig.path(handle.plan.blob_id)).expect("file"));
        plans.push(handle.plan);
    }
    assert_eq!(files[0].len(), files[1].len());
    assert_ne!(files[0][HEADER_LEN..], files[1][HEADER_LEN..]);
    assert_ne!(plans[0].wrapped_key, plans[1].wrapped_key);
    rig.finish().await;
}
