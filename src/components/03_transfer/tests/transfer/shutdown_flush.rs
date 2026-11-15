//! `PgUploadService::flush_all`, the upload checkpoint flush of `serve`'s shutdown (AZ-3791): every entry's
//! unsaved progress reaches its row at once, busy or idle, and the entries are evicted, so a restart loads them
//! from the rows.

use std::sync::Arc;
use std::time::Duration;

use contracts::Janitor;
use sc_clock::Clock;
use sunnycloud_transfer::IdleFlush;

use crate::janitor_support::short_config;
use crate::put_support::{NINE_MIB, bit};
use crate::start_support::Rig;

#[tokio::test]
async fn flush_all_saves_and_evicts_every_entry_that_the_idle_flush_leaves() {
    // Arrange: two sessions with acknowledged blocks a moment ago, so neither is idle.
    let rig = Rig::with_config(short_config()).await;
    let first = rig.upload(NINE_MIB, 340).await;
    let second = rig.upload(NINE_MIB, 341).await;
    for index in 0..2 {
        rig.put(&first, index).await.expect("first block");
    }
    rig.put(&second, 3).await.expect("second block");
    let idle_now = IdleFlush::new(Arc::clone(&rig.service))
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("idle flush")
        .processed;

    // Act
    let evicted = rig
        .service
        .flush_all(rig.clock.now_utc())
        .await
        .expect("shutdown flush");
    let first_row = rig.row(first.id).await;
    let second_row = rig.row(second.id).await;
    let first_reloaded = rig
        .service
        .get_upload(rig.account, first.id)
        .await
        .expect("loaded from the checkpoint");
    let second_reloaded = rig
        .service
        .get_upload(rig.account, second.id)
        .await
        .expect("loaded from the checkpoint");

    // Assert
    assert_eq!((idle_now, evicted), (0, 2));
    assert!(bit(&first_row.0, 0) && bit(&first_row.0, 1) && !bit(&first_row.0, 2));
    assert_eq!(&first_row.1[..32], first.hash(0).as_bytes());
    assert!(bit(&second_row.0, 3) && !bit(&second_row.0, 0));
    assert_eq!(first_reloaded.need_blocks, vec![2, 3, 4]);
    assert_eq!(second_reloaded.need_blocks, vec![0, 1, 2, 4]);
    rig.finish().await;
}

#[tokio::test]
async fn flush_all_evicts_a_session_with_nothing_unsaved_without_a_write() {
    // Arrange
    let rig = Rig::with_config(short_config()).await;
    let upload = rig.upload(NINE_MIB, 342).await;
    let before = rig.row(upload.id).await;
    rig.clock.tick(Duration::from_millis(10));

    // Act
    let evicted = rig
        .service
        .flush_all(rig.clock.now_utc())
        .await
        .expect("shutdown flush");
    let again = rig
        .service
        .flush_all(rig.clock.now_utc())
        .await
        .expect("second flush");

    // Assert
    assert_eq!((evicted, again), (1, 0));
    assert_eq!(rig.row(upload.id).await, before);
    rig.finish().await;
}
