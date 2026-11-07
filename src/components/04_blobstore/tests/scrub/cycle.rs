//! AC-5: a scheduled cycle that stops before `done` loses nothing; the next one recomputes its set from
//! `last_scrubbed_at`.

use std::time::Duration;

use contracts::{Janitor, ScrubMode, Scrubber};
use sc_clock::Clock;
use sunnycloud_blobstore::ScrubJanitor;

use crate::support::Rig;

const BLOBS: usize = 7;

#[tokio::test(flavor = "multi_thread")]
async fn a_cycle_ended_by_its_budget_is_finished_by_the_next_one_without_repeating_work() {
    // Arrange
    let rig = Rig::new().await;
    let mut ids = Vec::new();
    for seed in 0..BLOBS {
        ids.push(rig.store(64 * 1024, seed as u8).await.blob_id);
    }
    let mut short = rig.config.clone();
    short.scrub_cycle_budget = Duration::from_millis(1);
    let scrub = rig.scrub_with(short.clone(), 2, None);
    let impatient = ScrubJanitor::new(&short, scrub.scrubber.clone());
    let patient = ScrubJanitor::new(&rig.config, scrub.scrubber.clone());

    // Act
    let first = impatient
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("first cycle");
    let mut kept = Vec::new();
    for id in &ids {
        kept.push(rig.scrubbed_micros(*id).await);
    }
    let second = patient
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("second cycle");

    // Assert
    assert!(!first.done, "the 1 ms budget ends the cycle");
    assert!(first.checked >= 1 && (first.checked as usize) < BLOBS);
    assert!(second.done);
    assert_eq!(first.checked + second.checked, BLOBS as u64);
    for (id, before) in ids.iter().zip(kept) {
        let after = rig.scrubbed_micros(*id).await;
        assert!(after.is_some());
        if before.is_some() {
            assert_eq!(after, before, "progress of the first cycle is kept");
        }
    }
    assert_eq!(
        rig.count("SELECT count(*) FROM audit_log WHERE event = 'scrub_summary'")
            .await,
        2
    );
    drop((impatient, patient, scrub));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_janitor_runs_a_due_cycle_through_the_port_and_names_itself() {
    // Arrange
    let rig = Rig::new().await;
    for seed in 0..3 {
        rig.store(64 * 1024, seed).await;
    }
    let scrub = rig.scrub(10);
    let janitor = ScrubJanitor::new(&rig.config, scrub.scrubber.clone());
    let port: &dyn Janitor = &janitor;

    // Act
    let stats = port
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("cycle through the port");
    let again = port
        .run_cycle(rig.clock.now_utc())
        .await
        .expect("second cycle");
    let abandoned = scrub
        .scrubber
        .start_run(ScrubMode::Due, None)
        .await
        .expect("run");
    let nothing_due = scrub.scrubber.run_step(&abandoned).await.expect("step");

    // Assert
    assert_eq!(port.name(), "scrub_janitor");
    assert_eq!(stats.processed, 3);
    assert_eq!(
        again.processed, 0,
        "everything was scrubbed within the interval"
    );
    assert!(nothing_due.done);
    drop((janitor, scrub));
    rig.finish().await;
}
