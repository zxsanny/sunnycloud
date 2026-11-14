//! `IdleFlush` (`ProgressTable::flush_idle`; AZ-3756 AC-4, IT-03-10): entries with no block for
//! `checkpoint_interval` get their `received` and `block_hashes` written to the row and are evicted; busy entries
//! stay; a cycle with nothing idle writes nothing.

use std::sync::Arc;
use std::time::Duration;

use contracts::Janitor;
use sc_clock::Clock;
use sc_observability::{BuildInfo, Metrics};
use sunnycloud_transfer::IdleFlush;

use crate::janitor_support::short_config;
use crate::put_support::{NINE_MIB, bit};
use crate::start_support::Rig;

fn metrics() -> Metrics {
    Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics")
}

#[tokio::test]
async fn idle_flush_checkpoints_and_evicts_only_the_idle_sessions() {
    // Arrange
    let mut rig = Rig::with_config(short_config()).await;
    let metrics = metrics();
    rig.service = Arc::new(rig.instance().with_metrics(&metrics).expect("series"));
    let flush = IdleFlush::new(Arc::clone(&rig.service));
    let idle = rig.upload(NINE_MIB, 330).await;
    let busy = rig.upload(NINE_MIB, 331).await;
    for index in 0..2 {
        rig.put(&idle, index).await.expect("idle block");
        rig.put(&busy, index).await.expect("busy block");
    }
    rig.clock.tick(Duration::from_millis(50));
    rig.put(&busy, 2).await.expect("busy block");
    rig.clock.tick(Duration::from_millis(60));
    let busy_before = rig.row(busy.id).await;

    // Act
    let first = flush.run_cycle(rig.clock.now_utc()).await.expect("first");
    let after_first = metrics.render().await.expect("render");
    let nothing_due = flush.run_cycle(rig.clock.now_utc()).await.expect("again");
    let idle_row = rig.row(idle.id).await;
    let busy_row = rig.row(busy.id).await;
    let reloaded = rig
        .service
        .get_upload(rig.account, idle.id)
        .await
        .expect("loaded from the checkpoint");

    // Assert
    assert_eq!((first.processed, nothing_due.processed), (1, 0));
    assert!(
        after_first.contains("upload_sessions_active 1"),
        "{after_first}"
    );
    assert!(bit(&idle_row.0, 0) && bit(&idle_row.0, 1) && !bit(&idle_row.0, 2));
    assert_eq!(&idle_row.1[..32], idle.hash(0).as_bytes());
    assert_eq!(&idle_row.1[32..64], idle.hash(1).as_bytes());
    let rolled = rig
        .clock
        .now_utc()
        .checked_add(crate::janitor_support::SHORT_TTL)
        .expect("expiry");
    assert_eq!(idle_row.2, rolled.as_unix_micros());
    assert_eq!(busy_row, busy_before, "the busy session's row is untouched");
    assert_eq!(reloaded.need_blocks, vec![2, 3, 4]);
    drop(flush);
    rig.finish().await;
}

#[tokio::test]
async fn idle_flush_evicts_a_session_with_nothing_unsaved_without_a_write() {
    // Arrange
    let mut rig = Rig::with_config(short_config()).await;
    let metrics = metrics();
    rig.service = Arc::new(rig.instance().with_metrics(&metrics).expect("series"));
    let flush = IdleFlush::new(Arc::clone(&rig.service));
    let upload = rig.upload(NINE_MIB, 332).await;
    let before = rig.row(upload.id).await;
    rig.clock.tick(Duration::from_millis(150));

    // Act
    let cycle = flush.run_cycle(rig.clock.now_utc()).await.expect("cycle");
    let rendered = metrics.render().await.expect("render");

    // Assert
    assert_eq!(cycle.processed, 1);
    assert!(rendered.contains("upload_sessions_active 0"), "{rendered}");
    assert_eq!(rig.row(upload.id).await, before);
    drop(flush);
    rig.finish().await;
}
