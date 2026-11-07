//! AC-2: ranges and `read_head` equal the source slices and read at most two chunks per 4 KiB. AC-3: unsatisfiable
//! input is typed and returns no bytes. The read-ahead window never exceeds `blob_read_ahead_chunks`.

use contracts::BlobError;

use crate::support::{MIB, Rig, consume, data, range};

const SEALED: u64 = 65_552;
const SIZE: usize = 3 * MIB;

#[tokio::test(flavor = "multi_thread")]
async fn ranges_equal_the_source_slices_and_a_4k_range_reads_at_most_two_chunks() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(0xA2, SIZE);
    let blob = rig.store(&source).await;
    let probe = rig.reader.probe();
    let small = [(0, 0), (SIZE - 1, SIZE - 1)]
        .into_iter()
        .chain([0, 65_534, 1_000_003].map(|start| (start, start + 4095)));
    let wide = [(65_000, 65_536 * 3 + 100), (0, SIZE - 1)];

    for (start, end) in small.chain(wide) {
        let reads = probe.chunk_reads();
        let bytes = probe.chunk_bytes();

        // Act
        let stream = rig.open(&blob, range(start, end)).await.expect("open");
        let consumed = consume(stream).await;

        // Assert
        assert!(
            consumed.error.is_none(),
            "{start}..={end}: {:?}",
            consumed.error
        );
        assert!(consumed.bytes == source[start..=end], "{start}..={end}");
        let chunk_reads = probe.chunk_reads() - reads;
        assert_eq!(probe.chunk_bytes() - bytes, chunk_reads * SEALED);
        if end - start < 4096 + 1 {
            assert!(chunk_reads <= 2, "{start}..={end}: {chunk_reads} chunks");
        }
    }
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_4k_range_across_a_chunk_edge_reads_exactly_the_two_chunks_it_touches() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(0xA3, SIZE);
    let blob = rig.store(&source).await;
    let probe = rig.reader.probe();

    // Act
    let stream = rig
        .open(&blob, range(65_534, 65_534 + 4095))
        .await
        .expect("open");
    let consumed = consume(stream).await;

    // Assert
    assert!(consumed.bytes == source[65_534..65_534 + 4096]);
    assert_eq!(probe.chunk_reads(), 2);
    assert_eq!(probe.chunk_bytes(), 2 * SEALED);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn read_head_returns_the_first_64_kib_and_less_for_a_shorter_request_or_blob() {
    // Arrange
    let rig = Rig::new().await;
    let source = data(0xA4, SIZE);
    let blob = rig.store(&source).await;
    let tiny = rig.store(&data(0xA5, 100)).await;
    let empty = rig.store(&data(0xA6, 0)).await;

    // Act
    let full = rig.head(&blob, 65_536).await.expect("head");
    let part = rig.head(&blob, 1000).await.expect("head");
    let short_blob = rig.head(&tiny, 65_536).await.expect("head");
    let empty_blob = rig.head(&empty, 65_536).await.expect("head");

    // Assert
    assert_eq!(full[..], source[..65_536]);
    assert_eq!(part[..], source[..1000]);
    assert_eq!(short_blob.len(), 100);
    assert!(empty_blob.is_empty());
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unsatisfiable_ranges_and_an_oversized_head_are_typed_and_read_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let blob = rig.store(&data(0xA7, SIZE)).await;
    let probe = rig.reader.probe();
    let cases = [
        (SIZE, SIZE),
        (SIZE, SIZE + 10),
        (0, SIZE),
        (10, SIZE + 1),
        (10, 9),
    ];

    for (start, end) in cases {
        // Act
        let result = rig.open(&blob, range(start, end)).await;

        // Assert
        assert!(
            matches!(result, Err(BlobError::RangeNotSatisfiable)),
            "{start}..={end}"
        );
    }
    let oversized = rig.head(&blob, 65_537).await;
    assert!(
        matches!(oversized, Err(BlobError::OutOfRange)),
        "{oversized:?}"
    );
    let negative = contracts::BlobReader::open(
        &rig.reader,
        blob.blob_id,
        &blob.wrapped_key,
        blob.master_key_id,
        -1,
        None,
    )
    .await;
    assert!(matches!(negative, Err(BlobError::OutOfRange)));
    assert_eq!(probe.chunk_reads(), 0);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_read_ahead_window_never_holds_more_chunks_than_the_configuration_allows() {
    // Arrange
    let window = 4;
    let rig = Rig::with_window(window).await;
    let source = data(0xA9, SIZE);
    let blob = rig.store(&source).await;
    let probe = rig.reader.probe();
    let mut ahead = probe.read_ahead();

    // Act
    let stream = rig.open(&blob, None).await.expect("open");
    ahead
        .wait_for(|queued| *queued == window as usize)
        .await
        .expect("window fills");
    let consumed = consume(stream).await;

    // Assert
    assert!(consumed.bytes == *source);
    assert_eq!(probe.high_water(), window as usize);
    rig.finish().await;
}
