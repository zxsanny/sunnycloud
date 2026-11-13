//! AC-4: a step reads no ciphertext while the activity gauge is above `scrub_pause_active_streams`, and goes on
//! when it drops. The gauge counts block writes and open downloads.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use contracts::{BlobReader as _, BlobWriter as _, ScrubMode, Scrubber};
use sc_clock::Clock;
use sunnycloud_blobstore::{FileBlobWriter, ScrubJanitor, SyncBarrier, SyncHook};

use crate::support::{Rig, run_to_done};

const SETTLE: Duration = Duration::from_millis(250);

#[tokio::test(flavor = "multi_thread")]
async fn a_step_reads_nothing_while_the_gauge_is_above_the_limit_and_resumes_when_it_drops() {
    // Arrange
    let rig = Rig::new().await;
    for seed in 0..3 {
        rig.store(256 * 1024, seed).await;
    }
    let scrub = rig.scrub_with(rig.config.clone(), 10, Some(rig.gauge.clone()));
    let probe = scrub.scrubber.read_probe();
    let busy = (rig.gauge.enter(), rig.gauge.enter());
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("start");
    let step = {
        let (scrubber, run) = (Arc::clone(&scrub.scrubber), run);
        tokio::spawn(async move { scrubber.run_step(&run).await })
    };

    // Act
    tokio::time::sleep(SETTLE).await;
    let while_busy = (probe.chunk_reads(), step.is_finished());
    drop(busy.0);
    let still_busy = rig.gauge.active();
    drop(busy.1);
    let stepped = step.await.expect("step task").expect("step");

    // Assert
    assert_eq!(while_busy, (0, false));
    assert_eq!(still_busy, 1);
    assert_eq!(stepped.checked, 3);
    assert!(probe.chunk_reads() > 0);
    drop(scrub);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cycle_paused_until_its_budget_ends_without_reading_and_the_next_one_finishes() {
    // Arrange
    let rig = Rig::new().await;
    for seed in 0..3 {
        rig.store(64 * 1024, seed).await;
    }
    let mut config = rig.config.clone();
    config.scrub_cycle_budget = SETTLE;
    let scrub = rig.scrub_with(config.clone(), 10, Some(rig.gauge.clone()));
    let probe = scrub.scrubber.read_probe();
    let janitor = ScrubJanitor::new(&config, scrub.scrubber.clone());
    let patient = ScrubJanitor::new(&rig.config, scrub.scrubber.clone());
    let busy = (rig.gauge.enter(), rig.gauge.enter());

    // Act
    let paused = janitor
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("paused cycle");
    let reads = probe.chunk_reads();
    drop(busy);
    let resumed = patient
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("cycle after the load");

    // Assert
    assert_eq!((paused.checked, paused.done), (0, false));
    assert_eq!(reads, 0);
    assert_eq!((resumed.checked, resumed.done), (3, true));
    drop((janitor, patient, scrub));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scrubber_without_a_gauge_is_never_paused() {
    // Arrange
    let rig = Rig::new().await;
    rig.store(64 * 1024, 1).await;
    let scrub = rig.scrub(10);
    let busy = (rig.gauge.enter(), rig.gauge.enter(), rig.gauge.enter());
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("start");

    // Act
    let (total, _) = run_to_done(scrub.scrubber.as_ref(), &run, 5).await;

    // Assert
    assert_eq!(total.checked, 1);
    drop((busy, scrub));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_open_download_and_a_block_write_in_flight_each_count_in_the_gauge() {
    // Arrange
    let rig = Rig::new().await;
    let stored = rig.store(256 * 1024, 5).await;
    let hook = SyncHook::new();
    let writer = FileBlobWriter::new(
        &rig.config,
        Arc::clone(&rig.keyring),
        SyncBarrier::with_hook(Duration::ZERO, Arc::clone(&hook)),
    )
    .expect("writer")
    .with_activity(rig.gauge.clone());
    let plan = writer.plan(rig.account, 1024).expect("plan");
    let handle = writer.create_file(plan, 1024, false).await.expect("create");
    let token = writer
        .write_block(&handle, 0, Bytes::from(vec![1; 1024]))
        .await
        .expect("block");
    hook.hold();

    // Act
    let download = rig
        .reader
        .open(
            stored.blob_id,
            &stored.wrapped_key,
            stored.master_key_id,
            stored.size,
            None,
        )
        .await
        .expect("open");
    let with_download = rig.gauge.active();
    drop(download);
    let waiting = tokio::spawn(async move { writer.await_durable(token).await });
    let mut with_write = 0;
    for _ in 0..200 {
        with_write = rig.gauge.active();
        if with_write == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    hook.release();
    waiting
        .await
        .expect("durable task")
        .expect("block is durable");

    // Assert
    assert_eq!(with_download, 1);
    assert_eq!(with_write, 1);
    assert_eq!(rig.gauge.active(), 0);
    rig.finish().await;
}
