//! AC-1 and AC-2: a run ends under live traffic, and real damage is reported once and summarised.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use contracts::{ScrubMode, Scrubber};

use crate::support::{MIB, Rig, run_to_done};

const BLOBS: usize = 300;
const BATCH: u32 = 32;

#[tokio::test(flavor = "multi_thread")]
async fn a_run_ends_while_blobs_keep_arriving_and_reports_exactly_the_damaged_ones() {
    // Arrange
    let rig = Arc::new(Rig::new().await);
    let mut stores = tokio::task::JoinSet::new();
    for seed in 0..BLOBS {
        let rig = Arc::clone(&rig);
        stores.spawn(async move { rig.store(MIB, seed as u8).await.blob_id });
    }
    let mut ids = Vec::new();
    while let Some(stored) = stores.join_next().await {
        ids.push(stored.expect("store task"));
    }
    let (flipped, deleted) = (&ids[0..3], &ids[3..5]);
    flipped.iter().for_each(|id| rig.flip_bit(*id));
    deleted.iter().for_each(|id| rig.delete_file(*id));
    let scrub = rig.scrub(BATCH);
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("start");
    let stop = Arc::new(AtomicBool::new(false));
    let arrived = Arc::new(AtomicU64::new(0));
    let writer = {
        let (rig, stop, arrived) = (Arc::clone(&rig), Arc::clone(&stop), Arc::clone(&arrived));
        tokio::spawn(async move {
            while !stop.load(Ordering::SeqCst) {
                rig.store(64 * 1024, 9).await;
                arrived.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
    };

    // Act
    let (total, steps) = run_to_done(scrub.scrubber.as_ref(), &run, BLOBS).await;
    stop.store(true, Ordering::SeqCst);
    writer.await.expect("writer task");
    scrub.scrubber.finish_run(&run).await.expect("finish");
    let status = scrub.scrubber.status().await.expect("status");

    // Assert
    let arrived = arrived.load(Ordering::SeqCst);
    assert!(arrived > 0, "the writer committed while the run was going");
    assert!(total.done);
    assert!(steps <= BLOBS.div_ceil(BATCH as usize) + 1, "{steps} steps");
    assert_eq!(
        (total.checked, total.mismatched, total.missing),
        (BLOBS as u64, 3, 2)
    );
    for id in flipped.iter().chain(deleted) {
        assert!(rig.unavailable(*id).await);
        assert_eq!(rig.events(*id).await, 1);
    }
    assert_eq!(rig.all_events().await, 5);
    assert_eq!(
        rig.count(
            "SELECT count(*) FROM blobs WHERE last_scrubbed_at IS NULL AND state = 'committed'"
        )
        .await,
        arrived as i64,
        "the new blobs are left for the next run"
    );
    assert_eq!(
        rig.count(
            "SELECT count(*) FROM audit_log WHERE event = 'scrub_summary' \
             AND (detail->>'checked')::int = 300 AND (detail->>'mismatched')::int = 3 \
             AND (detail->>'missing')::int = 2 AND (detail->>'duration_ms')::bigint >= 0"
        )
        .await,
        1
    );
    assert_eq!(
        rig.count("SELECT count(*) FROM audit_log WHERE event = 'scrub_summary'")
            .await,
        1
    );
    assert!(status.oldest_scrubbed_at.is_some());
    assert_eq!(status.unscrubbed, arrived);
    assert_eq!(status.unavailable, 5);
    let summary = status.latest_summary.expect("latest summary");
    assert_eq!(
        (summary.checked, summary.mismatched, summary.missing),
        (BLOBS as u64, 3, 2)
    );
    let text = scrub.metrics.render().await.expect("render");
    for line in [
        "sunnycloud_scrub_blobs_verified_total 300",
        "sunnycloud_scrub_mismatches_total 3",
        "sunnycloud_scrub_missing_total 2",
        "sunnycloud_scrub_running 0",
    ] {
        assert!(text.lines().any(|l| l == line), "{line} in\n{text}");
    }
    drop(scrub);
    Arc::into_inner(rig)
        .expect("rig is not shared")
        .finish()
        .await;
}
