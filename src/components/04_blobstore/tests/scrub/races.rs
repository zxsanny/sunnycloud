//! AC-80: the scrub never reports a blob that is being uploaded or removed, and still reports real damage once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{ScrubMode, Scrubber, async_trait};
use sc_clock::Clock;
use sunnycloud_blobstore::{GcHook, GcHookPoint, GcHookStop, ScrubHook, ScrubPoint};
use tokio::sync::Barrier;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::support::{Rig, run_to_done};

/// Holds the sweeper's batch after its files are parked and before its rows are settled.
struct HoldSettle {
    barrier: Barrier,
}

#[async_trait]
impl GcHook for HoldSettle {
    async fn at(&self, point: GcHookPoint) -> Result<(), GcHookStop> {
        if point == GcHookPoint::BeforeSettle {
            self.barrier.wait().await;
            self.barrier.wait().await;
        }
        Ok(())
    }
}

/// What the collector does at which moment of the scrub's check of one blob.
enum Collector {
    /// After the file was judged: queue the blob for removal; with `finish`, let the sweeper remove it too.
    AfterVerified { finish: bool },
    /// Before the file is opened: the sweeper parks it and holds there. With `reinstated` the row is made
    /// `committed` again at that moment, as a reinstate that wins the race against the sweeper does.
    ParksFile { reinstated: bool },
}

struct Interference {
    rig: Arc<Rig>,
    action: Collector,
    hold: Arc<HoldSettle>,
    sweeping: Mutex<Option<JoinHandle<()>>>,
    fired: AtomicBool,
}

impl Interference {
    fn new(rig: &Arc<Rig>, action: Collector) -> Arc<Self> {
        Arc::new(Interference {
            rig: Arc::clone(rig),
            action,
            hold: Arc::new(HoldSettle {
                barrier: Barrier::new(2),
            }),
            sweeping: Mutex::new(None),
            fired: AtomicBool::new(false),
        })
    }

    /// The sweeper is parked at its settle; the test lets it go and waits for its cycle.
    async fn release_sweeper(&self) {
        self.hold.barrier.wait().await;
        let handle = self
            .sweeping
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            handle.await.expect("sweeper task");
        }
    }

    async fn make_committed_again(&self, blob_id: Uuid) {
        let mut conn = self.rig.owner().await;
        sqlx::query("DELETE FROM blob_gc WHERE blob_id = $1")
            .bind(blob_id)
            .execute(&mut conn)
            .await
            .expect("drop queue row");
        sqlx::query("UPDATE blobs SET state = 'committed' WHERE id = $1")
            .bind(blob_id)
            .execute(&mut conn)
            .await
            .expect("restore state");
        sqlx::Connection::close(conn).await.expect("close");
    }
}

#[async_trait]
impl ScrubHook for Interference {
    async fn at(&self, point: ScrubPoint) {
        let applies = matches!(
            (&self.action, point),
            (Collector::AfterVerified { .. }, ScrubPoint::Verified(_))
                | (Collector::ParksFile { .. }, ScrubPoint::Opening(_))
        );
        if !applies || self.fired.swap(true, Ordering::SeqCst) {
            return;
        }
        match (&self.action, point) {
            (Collector::AfterVerified { finish }, ScrubPoint::Verified(id)) => {
                self.rig.schedule_removal(id).await;
                if *finish {
                    let now = self.rig.clock.now_utc();
                    self.rig.sweeper(None).run_cycle(now).await.expect("sweep");
                }
            }
            (Collector::ParksFile { reinstated }, ScrubPoint::Opening(id)) => {
                self.rig.schedule_removal(id).await;
                let sweeper = self.rig.sweeper(Some(self.hold.clone()));
                let now = self.rig.clock.now_utc();
                let handle = tokio::spawn(async move {
                    sweeper.run_cycle(now).await.expect("sweep");
                });
                *self.sweeping.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
                self.hold.barrier.wait().await;
                if *reinstated {
                    self.make_committed_again(id).await;
                }
            }
            _ => {}
        }
    }
}

async fn quiet(rig: &Rig, id: Uuid) {
    assert!(!rig.unavailable(id).await, "marked unavailable");
    assert_eq!(rig.events(id).await, 0, "audit event written");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blob_still_uploading_is_never_selected() {
    // Arrange
    let rig = Rig::new().await;
    let uploading = rig.half_uploaded().await;
    let committed = rig.store(64 * 1024, 1).await.blob_id;
    let scrub = rig.scrub(10);
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("start");

    // Act
    let (total, _) = run_to_done(scrub.scrubber.as_ref(), &run, 5).await;

    // Assert
    assert_eq!((total.checked, total.mismatched, total.missing), (1, 0, 0));
    assert!(!rig.scrubbed(uploading).await);
    assert!(rig.scrubbed(committed).await);
    quiet(&rig, uploading).await;
    drop(scrub);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_file_whose_blob_is_queued_or_removed_after_the_read_is_not_reported() {
    for finish in [false, true] {
        // Arrange
        let rig = Arc::new(Rig::new().await);
        let blob = rig.store(64 * 1024, 2).await.blob_id;
        rig.flip_bit(blob);
        let hooked = hooked_scrubber(
            &rig,
            Interference::new(&rig, Collector::AfterVerified { finish }),
        );
        let run = hooked
            .start_run(ScrubMode::Full, None)
            .await
            .expect("start");

        // Act
        let (total, _) = run_to_done(hooked.as_ref(), &run, 5).await;

        // Assert
        assert_eq!(
            (total.mismatched, total.missing),
            (0, 0),
            "finish = {finish}"
        );
        quiet(&rig, blob).await;
        drop(hooked);
        Arc::into_inner(rig)
            .expect("rig is not shared")
            .finish()
            .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_the_sweeper_has_parked_is_not_missing_whether_the_row_is_queued_or_reinstated() {
    for reinstated in [false, true] {
        // Arrange
        let rig = Arc::new(Rig::new().await);
        let blob = rig.store(64 * 1024, 3).await.blob_id;
        let interference = Interference::new(&rig, Collector::ParksFile { reinstated });
        let scrubber = hooked_scrubber(&rig, Arc::clone(&interference));
        let run = scrubber
            .start_run(ScrubMode::Full, None)
            .await
            .expect("start");

        // Act
        let (during, _) = run_to_done(scrubber.as_ref(), &run, 5).await;
        let parked_while_scrubbing = rig.parked(blob).exists();
        interference.release_sweeper().await;

        // Assert
        assert!(parked_while_scrubbing);
        assert_eq!(
            (during.checked, during.mismatched, during.missing),
            (0, 0, 0),
            "reinstated = {reinstated}"
        );
        quiet(&rig, blob).await;
        if reinstated {
            assert!(rig.path(blob).exists(), "the sweeper put the file back");
            let again = scrubber
                .start_run(ScrubMode::Full, None)
                .await
                .expect("second run");
            let (after, _) = run_to_done(scrubber.as_ref(), &again, 5).await;
            assert_eq!((after.checked, after.mismatched, after.missing), (1, 0, 0));
            quiet(&rig, blob).await;
        }
        drop((scrubber, interference));
        Arc::into_inner(rig)
            .expect("rig is not shared")
            .finish()
            .await;
    }
}

fn hooked_scrubber(rig: &Arc<Rig>, hook: Arc<Interference>) -> Arc<dyn Scrubber> {
    let metrics = sc_observability::Metrics::new(sc_observability::BuildInfo {
        version: "test",
        commit: "test",
        test_clock: false,
    })
    .expect("metrics");
    let series = sunnycloud_blobstore::ScrubMetrics::register(&metrics).expect("series");
    Arc::new(
        sunnycloud_blobstore::BlobScrubber::new(
            &rig.config,
            Arc::clone(&rig.keyring),
            rig.api.clone(),
            rig.audit.clone(),
            rig.clock.clone(),
            series,
            10,
        )
        .expect("scrubber")
        .with_hook(hook),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn two_hundred_races_between_the_scrub_and_the_collector_report_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let scrub = rig.scrub(1);
    let sweeper = rig.sweeper(None);
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next_delay = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        Duration::from_micros(state % 3000)
    };

    // Act
    let mut ids = Vec::new();
    for round in 0..200u8 {
        let blob = rig.store(64 * 1024, round).await.blob_id;
        ids.push(blob);
        let (scrub_delay, gc_delay) = (next_delay(), next_delay());
        let scrubbing = async {
            tokio::time::sleep(scrub_delay).await;
            let run = scrub
                .scrubber
                .start_run(ScrubMode::Full, None)
                .await
                .expect("start");
            scrub.scrubber.run_step(&run).await.expect("step");
            scrub.scrubber.finish_run(&run).await.expect("finish");
        };
        let collecting = async {
            tokio::time::sleep(gc_delay).await;
            rig.schedule_removal(blob).await;
            sweeper.run_cycle(rig.clock.now_utc()).await.expect("sweep");
        };
        tokio::join!(scrubbing, collecting);
    }

    // Assert
    assert_eq!(rig.all_events().await, 0);
    assert_eq!(
        rig.count("SELECT count(*) FROM blobs WHERE unavailable_at IS NOT NULL")
            .await,
        0
    );
    assert_eq!(
        rig.count("SELECT count(*) FROM blobs").await,
        0,
        "all removed"
    );
    drop((scrub, sweeper));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn real_corruption_is_reported_once_however_many_runs_find_it() {
    // Arrange
    let rig = Rig::new().await;
    let bad = rig.store(64 * 1024, 4).await.blob_id;
    let good = rig.store(64 * 1024, 5).await.blob_id;
    rig.flip_bit(bad);
    let scrub = rig.scrub(10);

    // Act
    let mut totals = Vec::new();
    for _ in 0..2 {
        let run = scrub
            .scrubber
            .start_run(ScrubMode::Full, None)
            .await
            .expect("start");
        let (total, _) = run_to_done(scrub.scrubber.as_ref(), &run, 5).await;
        scrub.scrubber.finish_run(&run).await.expect("finish");
        totals.push((total.checked, total.mismatched, total.missing));
    }

    // Assert
    assert_eq!(totals, vec![(2, 1, 0), (2, 1, 0)]);
    assert!(rig.unavailable(bad).await);
    assert_eq!(rig.events(bad).await, 1);
    quiet(&rig, good).await;
    drop(scrub);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blob_whose_master_key_is_unknown_stops_the_step_and_is_not_reported() {
    // Arrange
    let rig = Rig::new().await;
    let blob = rig.store(64 * 1024, 6).await.blob_id;
    let mut conn = rig.owner().await;
    sqlx::query("UPDATE blobs SET master_key_id = 99 WHERE id = $1")
        .bind(blob)
        .execute(&mut conn)
        .await
        .expect("unknown key id");
    sqlx::Connection::close(conn).await.expect("close");
    let scrub = rig.scrub(10);
    let run = scrub
        .scrubber
        .start_run(ScrubMode::Full, None)
        .await
        .expect("start");

    // Act
    let step = scrub.scrubber.run_step(&run).await;

    // Assert
    assert!(step.is_err());
    quiet(&rig, blob).await;
    assert!(!rig.scrubbed(blob).await);
    drop(scrub);
    rig.finish().await;
}
