//! PT-03-4 (AZ-3759 AC-5; AC-43, AC-2): 10,000 random 4 KiB ranges from 16 clients on a 1 GiB dense file (DS-25),
//! timed from `open_revision_content` to the first chunk, then one 5 GiB stream read to its end. Both files go
//! through the real upload path first; only the reads are timed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{
    ByteRange, DownloadOptions, DownloadService, DownloadStatus, PlaintextStream, RevisionOrCurrent,
};
use sunnycloud_transfer::PgUploadService;
use uuid::Uuid;

use crate::load::{Blocks, host, mb_per_s, percentile, stored_file};
use crate::start_support::{GIB, Rig};

const CLIENTS: u64 = 16;
const READS: u64 = 10_000;
const READ: u64 = 4096;
const STREAM: u64 = 5 * GIB as u64;

async fn next_chunk(body: &mut PlaintextStream) -> Option<usize> {
    match std::future::poll_fn(|cx| body.as_mut().poll_next(cx)).await {
        Some(Ok(chunk)) => Some(chunk.len()),
        Some(Err(_)) | None => None,
    }
}

/// One client's reads at xorshift offsets: each read's time to its first chunk and the reads that failed.
async fn client(
    service: Arc<PgUploadService>,
    account: Uuid,
    node: Uuid,
    seed: u64,
) -> (Vec<Duration>, u64) {
    let downloads: &dyn DownloadService = service.as_ref();
    let mut state = (seed + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let (mut times, mut errors) = (Vec::new(), 0);
    for _ in 0..READS / CLIENTS {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let start = state % (GIB as u64 - READ);
        let options = DownloadOptions {
            range: Some(ByteRange {
                start,
                end: start + READ - 1,
            }),
            ..DownloadOptions::default()
        };
        let began = Instant::now();
        let first = match downloads
            .open_revision_content(account, RevisionOrCurrent::Current(node), options)
            .await
        {
            Ok(mut download) if download.status == DownloadStatus::Partial => {
                next_chunk(&mut download.body).await
            }
            Ok(_) | Err(_) => None,
        };
        times.push(began.elapsed());
        if first.is_none() {
            errors += 1;
        }
    }
    (times, errors)
}

/// PT-03-4 on REF-SRV only: range p95 (open plus first chunk) at most 4 ms (fail 5 ms); streaming at least
/// 250 MB/s (fail 112 MB/s).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-03-4 range latency and streaming rate are measured on REF-SRV only"]
async fn pt_03_4_random_ranges_are_fast_and_a_long_stream_keeps_line_rate() {
    // Arrange
    let rig = Rig::for_perf().await;
    let blocks = Arc::new(Blocks::dense(64));
    let (account, root) = (rig.account, rig.root);
    let ranged = stored_file(&rig.service, account, root, "ds25.bin", GIB as u64, &blocks).await;
    let streamed = stored_file(&rig.service, account, root, "stream.bin", STREAM, &blocks).await;
    drop(blocks);

    // Act
    let clients: Vec<_> = (0..CLIENTS)
        .map(|seed| tokio::spawn(client(Arc::clone(&rig.service), account, ranged, seed)))
        .collect();
    let (mut times, mut errors) = (Vec::new(), 0);
    for handle in clients {
        let (part, failed) = handle.await.expect("client");
        times.extend(part);
        errors += failed;
    }
    let downloads: &dyn DownloadService = rig.service.as_ref();
    let began = Instant::now();
    let mut download = downloads
        .open_revision_content(
            account,
            RevisionOrCurrent::Current(streamed),
            DownloadOptions::default(),
        )
        .await
        .expect("open the stream");
    let mut streamed_bytes = 0u64;
    while let Some(len) = next_chunk(&mut download.body).await {
        streamed_bytes += len as u64;
    }
    let stream_time = began.elapsed();

    // Assert
    let range_p95 = percentile(&mut times, 95);
    let rate = mb_per_s(streamed_bytes, stream_time);
    println!(
        "PT-03-4 on {}: {} ranges, p95 {range_p95:?}, {errors} errors; stream {rate:.0} MB/s",
        host(),
        times.len()
    );
    assert_eq!((errors, streamed_bytes), (0, STREAM));
    assert!(
        range_p95 <= Duration::from_millis(5),
        "fail above 5 ms: {range_p95:?}"
    );
    assert!(rate >= 112.0, "fail below 112 MB/s: {rate:.0}");
    assert!(
        range_p95 <= Duration::from_millis(4),
        "target 4 ms: {range_p95:?}"
    );
    assert!(rate >= 250.0, "target 250 MB/s: {rate:.0}");
    drop(download);
    rig.finish().await;
}
