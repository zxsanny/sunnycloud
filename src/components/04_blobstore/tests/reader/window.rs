//! The read-ahead window (owner decision 2026-11-14): a stream holds its authenticated first chunk and nothing more
//! until that chunk is taken, then reads one chunk ahead and doubles the window, up to `blob_read_ahead_chunks`,
//! only while the consumer keeps pace. The window never exceeds the configuration.

use std::time::Duration;

use crate::support::{MIB, Rig, consume, data, next};

const SIZE: usize = 3 * MIB;
/// Long enough for a read-ahead task that is not held back to read several chunks of the scratch volume.
const IDLE: Duration = Duration::from_millis(300);
/// What a slow consumer spends on each chunk: far longer than one chunk read, so its next chunk is always ready.
const SLOW: Duration = Duration::from_millis(100);

#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_consumer_holds_its_first_chunk_and_at_most_one_more() {
    // Arrange
    let rig = Rig::with_window(16).await;
    let source = data(0xB1, SIZE);
    let blob = rig.store(&source).await;
    let probe = rig.reader.probe();

    // Act
    let mut stream = rig.open(&blob, None).await.expect("open");
    tokio::time::sleep(IDLE).await;
    let reads_unpolled = probe.chunk_reads();
    let head = next(&mut stream).await.expect("head").expect("head bytes");
    probe
        .read_ahead()
        .wait_for(|queued| *queued == 1)
        .await
        .expect("one chunk ahead");
    tokio::time::sleep(IDLE).await;
    let (reads_stalled, widest_stalled) = (probe.chunk_reads(), probe.widest_window());
    let rest = consume(stream).await;

    // Assert
    assert_eq!(
        reads_unpolled, 1,
        "an unpolled stream holds only its first chunk"
    );
    assert_eq!(reads_stalled, 2, "a stalled consumer holds one chunk ahead");
    assert_eq!(widest_stalled, 0, "a stalled consumer never widens");
    assert!(rest.error.is_none(), "{:?}", rest.error);
    assert!([&head[..], &rest.bytes[..]].concat() == *source);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_consumer_slower_than_the_disk_stays_at_one_chunk_ahead() {
    // Arrange
    let rig = Rig::with_window(16).await;
    let source = data(0xB2, MIB);
    let blob = rig.store(&source).await;
    let probe = rig.reader.probe();
    let mut ahead = probe.read_ahead();
    let mut stream = rig.open(&blob, None).await.expect("open");
    let mut bytes = Vec::with_capacity(MIB);

    // Act
    while bytes.len() < MIB {
        let chunk = next(&mut stream)
            .await
            .expect("chunk")
            .expect("chunk bytes");
        bytes.extend_from_slice(&chunk);
        if bytes.len() < MIB {
            ahead
                .wait_for(|queued| *queued == 1)
                .await
                .expect("the next read started");
            tokio::time::sleep(SLOW).await;
        }
    }
    let end = next(&mut stream).await;

    // Assert
    assert!(end.is_none());
    assert!(bytes == *source);
    assert_eq!(probe.widest_window(), 0);
    assert_eq!(probe.high_water(), 1);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_consumer_that_keeps_pace_widens_the_window_to_the_configured_size() {
    // Arrange
    let rig = Rig::with_window(16).await;
    let source = data(0xB3, SIZE);
    let blob = rig.store(&source).await;
    let probe = rig.reader.probe();

    // Act
    let stream = rig.open(&blob, None).await.expect("open");
    let consumed = consume(stream).await;

    // Assert
    assert!(consumed.bytes == *source);
    assert_eq!(probe.widest_window(), 16);
    assert!(probe.high_water() <= 16, "{}", probe.high_water());
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
    let mut stream = rig.open(&blob, None).await.expect("open");
    let mut bytes = Vec::with_capacity(SIZE);

    // Act
    while probe.widest_window() < window as usize {
        let chunk = next(&mut stream)
            .await
            .expect("chunk")
            .expect("chunk bytes");
        bytes.extend_from_slice(&chunk);
    }
    ahead
        .wait_for(|queued| *queued == window as usize)
        .await
        .expect("window fills");
    tokio::time::sleep(IDLE).await;
    let held = *ahead.borrow();
    let rest = consume(stream).await;

    // Assert
    assert_eq!(held, window as usize);
    assert!(rest.error.is_none(), "{:?}", rest.error);
    assert!([&bytes[..], &rest.bytes[..]].concat() == *source);
    assert_eq!(probe.high_water(), window as usize);
    rig.finish().await;
}
