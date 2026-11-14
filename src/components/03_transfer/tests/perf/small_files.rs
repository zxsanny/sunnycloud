//! PT-03-3 (AZ-3759 AC-4; AC-40): the server's share of 10,000 small files: `stage_small` of DS-01 sizes
//! (log-uniform 1 KiB to 1 MiB) at concurrency 16, then 500-entry `commit_batch` calls, 20 in all.
//!
//! One account holds at most `max_staged` (1,000) staged files, so the run goes in 10 rounds of 1,000 stages
//! followed by their 2 batches, on the production limit; the counts (10,000 stages, 20 batches) and the timed calls
//! are the specification's. File contents are generated before each round's clock starts.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use contracts::{
    BatchCommit, BatchEntry, BatchEntryResult, CommitActor, CommitService, UploadService,
};
use sc_ids::{Hash32, hash_block};
use sunnycloud_transfer::PgUploadService;
use uuid::Uuid;

use crate::load::{body, host, percentile};
use crate::put_support::content;
use crate::start_support::Rig;

const FILES: usize = 10_000;
const ROUND: usize = 1_000;
const BATCH: usize = 500;
const CONCURRENCY: usize = 16;

/// DS-01: log-uniform from 1 KiB to 1 MiB, seeded by the file's index.
fn ds01_size(index: usize) -> usize {
    let mut state = (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
    (1024.0 * 1024f64.powf(unit)) as usize
}

struct Small {
    key: Uuid,
    name: String,
    bytes: Bytes,
    hash: Hash32,
}

fn round_files(round: usize) -> Arc<Vec<Small>> {
    Arc::new(
        (round * ROUND..(round + 1) * ROUND)
            .map(|index| {
                let bytes = Bytes::from(content(20_000 + index as u64, ds01_size(index)));
                Small {
                    key: Uuid::new_v4(),
                    name: format!("small-{index}.bin"),
                    hash: hash_block(&bytes),
                    bytes,
                }
            })
            .collect(),
    )
}

/// Stages `files` on [`CONCURRENCY`] tasks; returns each call's time.
async fn stage_all(
    service: &Arc<PgUploadService>,
    account: Uuid,
    files: &Arc<Vec<Small>>,
) -> Vec<Duration> {
    let next = Arc::new(AtomicUsize::new(0));
    let tasks: Vec<_> = (0..CONCURRENCY)
        .map(|_| {
            let (service, files, next) =
                (Arc::clone(service), Arc::clone(files), Arc::clone(&next));
            tokio::spawn(async move {
                let uploads: &dyn UploadService = service.as_ref();
                let mut times = Vec::new();
                loop {
                    let Some(file) = files.get(next.fetch_add(1, Ordering::SeqCst)) else {
                        return times;
                    };
                    let began = Instant::now();
                    uploads
                        .stage_small(
                            account,
                            file.key,
                            Some(file.hash),
                            body(file.bytes.clone()),
                            file.bytes.len() as u64,
                        )
                        .await
                        .expect("staged");
                    times.push(began.elapsed());
                }
            })
        })
        .collect();
    let mut times = Vec::with_capacity(files.len());
    for task in tasks {
        times.extend(task.await.expect("stage task"));
    }
    times
}

fn batch(files: &[Small], parent: Uuid) -> BatchCommit {
    BatchCommit {
        commit_id: Uuid::new_v4(),
        entries: files
            .iter()
            .map(|file| BatchEntry {
                staged_key: file.key,
                parent_id: parent,
                name: file.name.clone(),
                base_revision: None,
                client_mtime: None,
                client_meta: None,
                import_hint: false,
            })
            .collect(),
    }
}

/// PT-03-3 on REF-SRV only: total server time at most 25 s (fail 45 s), `stage_small` p95 at most 40 ms (fail
/// 100 ms), batch p95 at most 1.0 s (fail 2 s); every file committed.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "NOT-RUN(HW-REF): PT-03-3 small-file server time is measured on REF-SRV only"]
async fn pt_03_3_ten_thousand_small_files_stage_and_commit_within_the_server_budget() {
    // Arrange
    let rig = Rig::for_perf().await;
    let commits: &dyn CommitService = rig.service.as_ref();
    let actor = CommitActor {
        account_id: rig.account,
        device_label: "perf".to_owned(),
    };
    let (mut stage_times, mut batch_times) = (Vec::with_capacity(FILES), Vec::new());
    let (mut server_time, mut committed) = (Duration::ZERO, 0);

    // Act
    for round in 0..FILES / ROUND {
        let files = round_files(round);
        let began = Instant::now();
        stage_times.extend(stage_all(&rig.service, rig.account, &files).await);
        server_time += began.elapsed();
        for chunk in files.chunks(BATCH) {
            let request = batch(chunk, rig.root);
            let began = Instant::now();
            let result = commits
                .commit_batch(actor.clone(), request)
                .await
                .expect("batch");
            let took = began.elapsed();
            server_time += took;
            batch_times.push(took);
            committed += result
                .per_entry
                .iter()
                .filter(|entry| matches!(entry, BatchEntryResult::Committed(_)))
                .count();
        }
    }

    // Assert
    let stage_p95 = percentile(&mut stage_times, 95);
    let batch_p95 = percentile(&mut batch_times, 95);
    println!(
        "PT-03-3 on {}: total {server_time:?}; stage p95 {stage_p95:?}; batch p95 {batch_p95:?}; {committed} committed",
        host()
    );
    assert_eq!((committed, batch_times.len()), (FILES, FILES / BATCH));
    assert!(
        server_time <= Duration::from_secs(45),
        "fail above 45 s: {server_time:?}"
    );
    assert!(
        stage_p95 <= Duration::from_millis(100),
        "fail above 100 ms: {stage_p95:?}"
    );
    assert!(
        batch_p95 <= Duration::from_secs(2),
        "fail above 2 s: {batch_p95:?}"
    );
    assert!(
        server_time <= Duration::from_secs(25),
        "target 25 s: {server_time:?}"
    );
    assert!(
        stage_p95 <= Duration::from_millis(40),
        "target 40 ms: {stage_p95:?}"
    );
    assert!(
        batch_p95 <= Duration::from_secs(1),
        "target 1.0 s: {batch_p95:?}"
    );
    rig.finish().await;
}
