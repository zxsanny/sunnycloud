//! PT-03-2 (AZ-3759 AC-3; AC-4): `commit_upload` time does not grow with the file. 20 commits each of 1 MiB,
//! 1 GiB and 8 GiB sparse-hybrid files (DS-10 slices); only the commit call is timed. Every file has an account
//! of its own, so the 10 GiB default quota holds an 8 GiB file. Needs about 190 GiB on the blob volume (C-LARGE).
//! The I/O half (one head read whatever the size) runs everywhere: `transfer::commit_constant`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::load::{Blocks, commit_file, host, percentile, send_file};
use crate::start_support::{GIB, MIB, Rig};

const REPEATS: usize = 20;
const SIZES: [i64; 3] = [MIB, GIB, 8 * GIB];

/// PT-03-2 on the C-LARGE host only: commit p95 at most 100 ms (fail 2 s); the p95 of 8 GiB commits within 20%
/// of the p95 of 1 MiB commits (fail 50%).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "NOT-RUN(HW-REF): PT-03-2 commit time is measured on the C-LARGE host only"]
async fn pt_03_2_commit_time_does_not_grow_with_the_file() {
    // Arrange
    let rig = Rig::for_perf().await;
    let blocks = Arc::new(Blocks::sparse_hybrid(64));
    let mut times: Vec<Vec<Duration>> = Vec::with_capacity(SIZES.len());

    // Act
    for size in SIZES {
        let size = u64::try_from(size).expect("size");
        let mut per_size = Vec::with_capacity(REPEATS);
        for repeat in 0..REPEATS {
            let (account, root) = rig.other_account().await;
            let name = format!("commit-{size}-{repeat}.bin");
            let upload_id = send_file(&rig.service, account, root, &name, size, &blocks).await;
            let blocklist = blocks.blocklist(size);
            let began = Instant::now();
            commit_file(&rig.service, account, root, &name, upload_id, blocklist).await;
            per_size.push(began.elapsed());
        }
        times.push(per_size);
    }

    // Assert
    let mut all: Vec<Duration> = times.iter().flatten().copied().collect();
    let p95 = percentile(&mut all, 95);
    let smallest = percentile(&mut times[0], 95);
    let middle = percentile(&mut times[1], 95);
    let largest = percentile(&mut times[2], 95);
    let spread = (largest.as_secs_f64() - smallest.as_secs_f64()).abs() / smallest.as_secs_f64();
    println!(
        "PT-03-2 on {}: p95 {p95:?}; 1 MiB {smallest:?}, 1 GiB {middle:?}, 8 GiB {largest:?}; spread {:.0}%",
        host(),
        spread * 100.0
    );
    assert!(p95 <= Duration::from_secs(2), "fail above 2 s: {p95:?}");
    assert!(spread <= 0.5, "fail above 50%: {:.0}%", spread * 100.0);
    assert!(p95 <= Duration::from_millis(100), "target 100 ms: {p95:?}");
    assert!(spread <= 0.2, "target 20%: {:.0}%", spread * 100.0);
    rig.finish().await;
}
