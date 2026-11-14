//! PT-03-5, download half (AZ-3759 AC-6; AC-51; env C): streaming a 5 GiB file grows the process by at most 16 MiB
//! (fail 64 MiB). Its own test binary with one test, so no other test allocates while memory is measured; heap bytes
//! are counted at the allocator (as in `range_memory.rs`) and the resident set is read from `/proc/self/status`.
//! The progress-table half of PT-03-5 is `progress_memory.rs`.
//!
//! The file goes through the real upload path (`start_upload`, `put_block`, `commit_upload`) on the real 02, 04,
//! 05 and 07a; the body is read chunk by chunk and each chunk dropped, as 11 hands it to the socket.
//!
//! Ignored by default: writing and reading 5 GiB through the debug-build cipher takes about an hour (1 GiB written
//! takes about 6 minutes in `range_memory`). Run it in release:
//! `cargo test --release -p sunnycloud-transfer --test download_memory -- --ignored --nocapture`.

#[allow(dead_code, unused_imports)]
#[path = "transfer/commit_ports.rs"]
mod commit_ports;
#[allow(dead_code, unused_imports)]
#[path = "perf/load.rs"]
mod load;
#[allow(dead_code, unused_imports)]
#[path = "transfer/put_support.rs"]
mod put_support;
#[allow(dead_code, unused_imports)]
#[path = "transfer/rig_config.rs"]
mod rig_config;
#[allow(dead_code, unused_imports)]
#[path = "transfer/rig_wiring.rs"]
mod rig_wiring;
#[allow(dead_code, unused_imports)]
#[path = "transfer/start_ports.rs"]
mod start_ports;
#[allow(dead_code, unused_imports)]
#[path = "transfer/start_support.rs"]
mod start_support;
#[allow(dead_code, unused_imports)]
#[path = "transfer/support.rs"]
mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use contracts::{DownloadOptions, DownloadService, RevisionOrCurrent};

use crate::load::{Blocks, host, stored_file};
use crate::start_support::{GIB, Rig};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

impl Counting {
    fn grew(size: usize) {
        let now = LIVE.fetch_add(size, Ordering::SeqCst) + size;
        PEAK.fetch_max(now, Ordering::SeqCst);
    }
}

// SAFETY: every call is forwarded unchanged to the system allocator; the counters only observe sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Counting::grew(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        Counting::grew(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        Counting::grew(new_size);
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const SIZE: u64 = 5 * GIB as u64;
const LIMIT: usize = 16 << 20;
const FAIL_ABOVE: usize = 64 << 20;
/// Chunks between two reads of the resident set (64 KiB chunks: every 4 MiB).
const RSS_EVERY: u64 = 64;

fn rss_bytes() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("process status");
    let kib: usize = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .expect("VmRSS line");
    kib * 1024
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(time): 5 GiB through the debug-build cipher takes about an hour; run with --release --ignored"]
async fn pt_03_5_streaming_five_gibibytes_keeps_memory_growth_bounded() {
    // Arrange
    let rig = Rig::for_perf().await;
    let blocks = Arc::new(Blocks::dense(64));
    let node = stored_file(
        &rig.service,
        rig.account,
        rig.root,
        "five.bin",
        SIZE,
        &blocks,
    )
    .await;
    drop(blocks);
    let downloads: &dyn DownloadService = rig.service.as_ref();
    let (heap_before, rss_before) = (LIVE.load(Ordering::SeqCst), rss_bytes());
    PEAK.store(heap_before, Ordering::SeqCst);

    // Act
    let mut download = downloads
        .open_revision_content(
            rig.account,
            RevisionOrCurrent::Current(node),
            DownloadOptions::default(),
        )
        .await
        .expect("open");
    let (mut bytes, mut chunks, mut rss_peak) = (0u64, 0u64, rss_before);
    while let Some(chunk) = std::future::poll_fn(|cx| download.body.as_mut().poll_next(cx)).await {
        bytes += chunk.expect("authentic chunk").len() as u64;
        chunks += 1;
        if chunks.is_multiple_of(RSS_EVERY) {
            rss_peak = rss_peak.max(rss_bytes());
        }
    }
    drop(download);
    let heap = PEAK.load(Ordering::SeqCst).saturating_sub(heap_before);
    let rss = rss_peak.saturating_sub(rss_before);

    // Assert
    eprintln!(
        "PT-03-5 download on {}: {bytes} B in {chunks} chunks; heap peak growth {heap} B; RSS peak growth {rss} B",
        host()
    );
    assert_eq!(bytes, SIZE);
    assert!(
        heap <= FAIL_ABOVE,
        "heap growth {heap} B, fail above {FAIL_ABOVE} B"
    );
    assert!(
        rss <= FAIL_ABOVE,
        "RSS growth {rss} B, fail above {FAIL_ABOVE} B"
    );
    assert!(heap <= LIMIT, "heap growth {heap} B, target {LIMIT} B");
    assert!(rss <= LIMIT, "RSS growth {rss} B, target {LIMIT} B");
    rig.finish().await;
}
