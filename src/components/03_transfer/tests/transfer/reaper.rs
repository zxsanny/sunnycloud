//! `SessionReaper` (AZ-3756 AC-1, AC-6; IT-03-10, IT-03-17 case a) and the one expiry rule at the key: an expired
//! row under a `client_key` or `staged_key` is reaped by the next start or stage with that key.

use std::sync::Arc;
use std::time::Duration;

use contracts::{Janitor, UploadError};
use sc_clock::Clock;
use uuid::Uuid;

use crate::janitor_support::short_config;
use crate::put_support::content;
use crate::start_support::{MIB, Rig, request, started, ticks_past_ttl};

const SESSION: usize = 3 * MIB as usize;

#[tokio::test]
async fn session_reaper_releases_exactly_the_expired_rows_once_and_discards_after_commit() {
    // Arrange
    let rig = Rig::with_config(short_config()).await;
    let reaper = Arc::new(sunnycloud_transfer::SessionReaper::new(Arc::clone(
        &rig.service,
    )));
    let mut uploads = Vec::new();
    for seed in 0..20 {
        uploads.push(rig.upload(SESSION, 300 + seed).await);
    }
    let mut staged = Vec::new();
    let mut staged_bytes = 0i64;
    for index in 0..20 {
        let data = content(400 + index, 1000 + index as usize);
        let key = Uuid::new_v4();
        rig.stage(key, &data).await.expect("staged");
        staged_bytes += data.len() as i64;
        staged.push(key);
    }
    rig.clock.tick(Duration::from_millis(300));
    for upload in &uploads[..10] {
        rig.put(upload, 0).await.expect("touched");
    }
    rig.clock.tick(Duration::from_millis(150));
    let now = rig.clock.now_utc();
    let reserved = rig.reserved().await;
    let files = rig.files();
    let expired_bytes = 10 * SESSION as i64 + staged_bytes;
    rig.writer.hold_discards();

    // Act
    let cycle = {
        let reaper = Arc::clone(&reaper);
        tokio::spawn(async move { reaper.run_cycle(now).await })
    };
    rig.writer.wait_discards_parked(1).await;
    let while_held = (rig.sessions().await, rig.reserved().await, rig.files());
    rig.writer.release_discards();
    let first = cycle.await.expect("cycle task").expect("first cycle");
    let after_first = (rig.sessions().await, rig.reserved().await, rig.files());
    let second = reaper.run_cycle(now).await.expect("second cycle");

    // Assert
    assert_eq!(first.processed, 30);
    assert_eq!(
        while_held,
        (10, reserved - expired_bytes, files),
        "rows gone and reservation released before any file is discarded"
    );
    assert_eq!(after_first, (10, reserved - expired_bytes, 10));
    assert_eq!(second.processed, 0);
    assert_eq!(rig.reserved().await, reserved - expired_bytes);
    for upload in &uploads[..10] {
        assert!(rig.has_session(upload.id).await, "touched sessions survive");
    }
    for key in &staged {
        assert!(rig.staged_row(*key).await.is_none());
    }
    assert_eq!(rig.uploading_blobs().await, 10);
    drop(reaper);
    rig.finish().await;
}

#[tokio::test]
async fn session_reaper_an_acked_upload_never_committed_leaves_nothing_after_the_ttl() {
    // Arrange
    let mut rig = Rig::new().await;
    let upload = rig.upload(SESSION, 310).await;
    rig.send_all(&upload).await;
    rig.restart();
    ticks_past_ttl(&rig.clock);
    let reaper = sunnycloud_transfer::SessionReaper::new(Arc::clone(&rig.service));

    // Act
    let cycle = reaper.run_cycle(rig.clock.now_utc()).await.expect("cycle");

    // Assert
    assert_eq!(cycle.processed, 1);
    assert_eq!(rig.reserved().await, 0);
    assert_eq!(rig.revisions().await, 0);
    assert_eq!(rig.nodes_of(rig.account).await, 1, "the root only");
    assert_eq!((rig.sessions().await, rig.uploading_blobs().await), (0, 0));
    assert!(!rig.file_exists(upload.blob));
    drop(reaper);
    rig.finish().await;
}

#[tokio::test]
async fn session_reaper_a_cycle_with_nothing_due_writes_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(SESSION, 311).await;
    let reaper = sunnycloud_transfer::SessionReaper::new(Arc::clone(&rig.service));
    let reserved = rig.reserved().await;

    // Act
    let cycle = reaper.run_cycle(rig.clock.now_utc()).await.expect("cycle");

    // Assert
    assert_eq!(cycle.processed, 0);
    assert_eq!(rig.reserved().await, reserved);
    assert!(rig.has_session(upload.id).await);
    drop(reaper);
    rig.finish().await;
}

#[tokio::test]
async fn start_upload_an_expired_session_under_the_key_is_reaped_and_started_anew() {
    // Arrange
    let rig = Rig::new().await;
    let first = started(rig.start(request(rig.root, "a.bin", 4 * MIB, "k1")).await);
    let old_blob = rig.blob_of(first.upload_id).await;
    ticks_past_ttl(&rig.clock);

    // Act
    let again = started(rig.start(request(rig.root, "a.bin", 4 * MIB, "k1")).await);

    // Assert
    assert_ne!(again.upload_id, first.upload_id);
    assert_eq!(again.need_blocks, vec![0, 1]);
    assert_eq!(
        rig.reserved().await,
        4 * MIB,
        "the old reservation released once"
    );
    assert_eq!((rig.sessions().await, rig.uploading_blobs().await), (1, 1));
    assert!(!rig.file_exists(old_blob));
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_an_expired_staged_row_under_the_key_is_reaped_and_staged_anew() {
    // Arrange
    let rig = Rig::new().await;
    let key = Uuid::new_v4();
    let data = content(320, 4096);
    rig.stage(key, &data).await.expect("staged");
    let old_blob = rig.staged_row(key).await.expect("row").blob;
    ticks_past_ttl(&rig.clock);

    // Act
    let repeat = rig.stage(key, &data).await;

    // Assert
    assert!(repeat.is_ok(), "{repeat:?}");
    let row = rig.staged_row(key).await.expect("new row");
    assert_ne!(row.blob, old_blob);
    assert_eq!(
        rig.reserved().await,
        4096,
        "the old reservation released once"
    );
    assert!(!rig.file_exists(old_blob));
    assert!(rig.file_exists(row.blob));
    rig.finish().await;
}

#[tokio::test]
async fn stage_small_an_expired_staged_row_of_other_content_is_not_a_mismatch() {
    // Arrange
    let rig = Rig::new().await;
    let key = Uuid::new_v4();
    rig.stage(key, &content(321, 100)).await.expect("staged");
    ticks_past_ttl(&rig.clock);

    // Act
    let other = rig.stage(key, &content(322, 200)).await;

    // Assert
    assert!(
        !matches!(other, Err(UploadError::ClientKeyMismatch)),
        "{other:?}"
    );
    assert_eq!(rig.reserved().await, 200);
    rig.finish().await;
}
