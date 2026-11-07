//! A reinstate meeting the sweeper. `reinstate_in_tx` stats the file and then locks the row; the sweeper locks the
//! row when it claims and when it settles. Barriers in the hook put the reinstate at the exact points of a batch.

use std::time::Duration;

use contracts::{Due, GcQueue, GcReason, ReinstateOutcome};
use sc_db::{DbError, RunError, Scope};
use sunnycloud_blobstore::GcHookPoint;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::rows::START;
use crate::support::{Pause, Scene, after, gc_config};

const SECOND: Duration = Duration::from_secs(1);
const LEASE: Duration = Duration::from_secs(60);

async fn reinstate(scene: &Scene, blob: Uuid) -> ReinstateOutcome {
    scene
        .rig
        .api
        .run(Scope::Account(scene.account), async |tx| {
            scene.rig.registry.reinstate_in_tx(tx, blob).await
        })
        .await
        .expect("reinstate")
}

async fn assert_alive(scene: &Scene, blob: Uuid) {
    assert_eq!(
        scene.row_state(blob).await.as_deref(),
        Some("committed"),
        "the reinstated blob is committed"
    );
    assert!(scene.has_file(blob), "and its file is in place");
    assert!(!scene.has_parked_file(blob));
    assert_eq!(scene.queue_rows().await, 0);
}

#[tokio::test]
async fn a_blob_reinstated_after_the_claim_keeps_its_file() {
    // Arrange
    let scene = Scene::new().await;
    let blob = scene.queue(1, Due::Now).await[0];
    let pause = Pause::at(&[GcHookPoint::Claimed]);
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper(&config, Some(pause.clone()));

    // Act
    let (stats, outcome) = tokio::join!(Scene::run(&sweeper, after(START, SECOND)), async {
        pause.arrived().await;
        let outcome = reinstate(&scene, blob).await;
        pause.release().await;
        outcome
    });

    // Assert
    assert_eq!(outcome, ReinstateOutcome::Reinstated);
    assert_eq!(stats.removed, 0);
    assert_alive(&scene, blob).await;
    drop(sweeper);
    scene.finish().await;
}

const SETTLE_WAITING: &str = "SELECT count(*) FROM pg_stat_activity \
     WHERE datname = current_database() AND application_name = 'blob_gc_settle' AND wait_event_type = 'Lock'";

/// Returns once the settling transaction of the sweeper waits for a lock.
async fn settle_is_waiting(scene: &Scene) {
    let mut admin = scene.rig.db.admin().await;
    let mut polls = 0u32;
    loop {
        let waiting: i64 = sqlx::query_scalar(SETTLE_WAITING)
            .fetch_one(&mut admin)
            .await
            .expect("probe pg_stat_activity");
        if waiting == 1 {
            break;
        }
        polls += 1;
        assert!(
            polls < 100_000,
            "the settling transaction never waited for the row lock"
        );
        tokio::task::yield_now().await;
    }
    sqlx::Connection::close(admin).await.expect("close");
}

#[tokio::test]
async fn a_reinstate_that_straddles_the_file_step_is_waited_for_and_wins() {
    // Arrange
    let scene = Scene::new().await;
    let blob = scene.queue(1, Due::Now).await[0];
    let pause = Pause::at(&[GcHookPoint::Claimed, GcHookPoint::BeforeSettle]);
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper(&config, Some(pause.clone()));
    let (start_tx, start_rx) = oneshot::channel::<()>();
    let (restored_tx, restored_rx) = oneshot::channel();
    let (commit_tx, commit_rx) = oneshot::channel::<()>();

    // Act
    let (stats, outcome, ()) = tokio::join!(
        Scene::run(&sweeper, after(START, SECOND)),
        async {
            // Starts after the claim: it stats the file where it still is, locks the row and stays open.
            start_rx.await.expect("start");
            scene
                .rig
                .api
                .run(Scope::Account(scene.account), async |tx| {
                    let outcome = scene.rig.registry.reinstate_in_tx(tx, blob).await;
                    restored_tx.send(()).expect("announce");
                    commit_rx.await.expect("commit signal");
                    outcome
                })
                .await
                .expect("reinstate")
        },
        async {
            pause.arrived().await;
            start_tx.send(()).expect("start reinstate");
            restored_rx.await.expect("restored");
            pause.release().await;
            pause.arrived().await;
            assert!(!scene.has_file(blob) && scene.has_parked_file(blob));
            pause.release().await;
            settle_is_waiting(&scene).await;
            commit_tx.send(()).expect("commit");
        }
    );

    // Assert
    assert_eq!(stats.removed, 0);
    assert_eq!(outcome, ReinstateOutcome::Reinstated);
    assert_alive(&scene, blob).await;
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn a_reinstate_after_the_file_step_finds_the_file_missing_and_the_blob_goes() {
    // Arrange
    let scene = Scene::new().await;
    let blob = scene.queue(1, Due::Now).await[0];
    let pause = Pause::at(&[GcHookPoint::BeforeSettle]);
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper(&config, Some(pause.clone()));

    // Act
    let (stats, outcome) = tokio::join!(Scene::run(&sweeper, after(START, SECOND)), async {
        pause.arrived().await;
        let outcome = reinstate(&scene, blob).await;
        pause.release().await;
        outcome
    });

    // Assert
    assert_eq!(outcome, ReinstateOutcome::FileMissing);
    assert_eq!(stats.removed, 1);
    assert!(scene.row_state(blob).await.is_none());
    assert!(!scene.has_file(blob) && !scene.has_parked_file(blob));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn a_blob_queued_again_after_a_reinstate_is_not_removed_by_the_old_claim() {
    // Arrange
    let scene = Scene::new().await;
    let blob = scene.queue(1, Due::Now).await[0];
    let pause = Pause::at(&[GcHookPoint::Claimed]);
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper(&config, Some(pause.clone()));

    // Act
    let (stats, ()) = tokio::join!(Scene::run(&sweeper, after(START, SECOND)), async {
        pause.arrived().await;
        reinstate(&scene, blob).await;
        scene
            .rig
            .api
            .run(Scope::Account(scene.account), async |tx| {
                scene
                    .rig
                    .queue
                    .schedule_in_tx(tx, blob, GcReason::Replaced, Due::AfterRetention)
                    .await
            })
            .await
            .expect("queue again");
        pause.release().await;
    });

    // Assert
    assert_eq!(stats.removed, 0);
    assert_eq!(scene.row_state(blob).await.as_deref(), Some("gc_pending"));
    assert!(scene.has_file(blob) && !scene.has_parked_file(blob));
    assert_eq!(scene.queue_rows().await, 1);
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn the_claim_leaves_a_row_that_a_reinstate_holds_and_does_not_wait_for_it() {
    // Arrange
    let scene = Scene::new().await;
    let blob = scene.queue(1, Due::Now).await[0];
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper(&config, None);
    let (held_tx, held_rx) = oneshot::channel();
    let (done_tx, done_rx) = oneshot::channel::<()>();

    // Act
    let ((), stats) = tokio::join!(
        async {
            scene
                .rig
                .api
                .run(Scope::Account(scene.account), async |tx| {
                    let outcome = scene.rig.registry.reinstate_in_tx(tx, blob).await;
                    held_tx.send(()).expect("announce");
                    done_rx.await.expect("done signal");
                    outcome.map(|_| ()).map_err(|_| DbError::Unavailable)
                })
                .await
                .map_err(|error: RunError<DbError>| error)
                .expect("reinstate");
        },
        async {
            held_rx.await.expect("held");
            let stats = Scene::run(&sweeper, after(START, SECOND)).await;
            done_tx.send(()).expect("release");
            stats
        }
    );

    // Assert
    assert_eq!(stats.removed, 0);
    assert_alive(&scene, blob).await;
    drop(sweeper);
    scene.finish().await;
}
