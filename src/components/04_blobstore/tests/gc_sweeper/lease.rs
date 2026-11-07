//! Janitors share no state: a failing blob does not hold up its batch, two sweepers split a backlog, a cancelled
//! shutdown ends the cycle, and the `Janitor` port reports what was removed.

use std::time::Duration;

use contracts::{Due, Janitor};
use sc_clock::CancellationToken;
use sunnycloud_blobstore::GcStats;

use crate::rows::START;
use crate::support::{Scene, after, gc_config};

const SECOND: Duration = Duration::from_secs(1);
const LEASE: Duration = Duration::from_secs(60);

#[tokio::test]
async fn a_blob_whose_cleanup_fails_stays_while_its_batch_mates_go_and_is_retried_after_the_lease()
{
    // Arrange
    let scene = Scene::new().await;
    let ids = scene.queue(10, Due::Now).await;
    let bad = ids[3];
    scene.cleanup.refuse(bad);
    let sweeper = scene.sweeper(&gc_config(&scene.rig, 10, LEASE, 60 * SECOND), None);
    let now = after(START, SECOND);

    // Act
    let first = Scene::run(&sweeper, now).await;
    let state_after_failure = scene.row_state(bad).await;
    let file_back_after_failure = scene.has_file(bad) && !scene.has_parked_file(bad);
    scene.cleanup.allow_all();
    let retried = Scene::run(&sweeper, after(now, LEASE + SECOND)).await;

    // Assert
    assert_eq!(first.removed, 9);
    assert_eq!(state_after_failure.as_deref(), Some("gc_pending"));
    assert!(file_back_after_failure);
    assert_eq!(retried.removed, 1);
    assert_eq!(scene.rows().await, 0);
    assert!(
        ids.iter()
            .all(|id| !scene.has_file(*id) && !scene.has_parked_file(*id))
    );
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn two_sweepers_over_one_backlog_remove_each_blob_once() {
    // Arrange
    let scene = Scene::new().await;
    let ids = scene.queue(100, Due::Now).await;
    let config = gc_config(&scene.rig, 7, LEASE, 60 * SECOND);
    let (a, b) = (scene.sweeper(&config, None), scene.sweeper(&config, None));
    let now = after(START, SECOND);

    // Act
    let (first, second) = tokio::join!(Scene::run(&a, now), Scene::run(&b, now));

    // Assert
    assert_eq!(first.removed + second.removed, 100);
    for id in ids {
        assert_eq!(scene.cleanup.calls(id), Some((1, true)), "blob {id}");
    }
    assert_eq!(scene.rows().await, 0);
    drop((a, b));
    scene.finish().await;
}

#[tokio::test]
async fn a_cancelled_shutdown_ends_the_cycle_before_any_batch() {
    // Arrange
    let scene = Scene::new().await;
    scene.queue(5, Due::Now).await;
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper_with(&config, None, shutdown);

    // Act
    let stats = Scene::run(&sweeper, after(START, SECOND)).await;

    // Assert
    assert_eq!(
        stats,
        GcStats {
            removed: 0,
            remaining_due: 5,
            budget_spent: false
        }
    );
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn the_janitor_port_reports_the_removed_count_as_processed() {
    // Arrange
    let scene = Scene::new().await;
    scene.queue(12, Due::Now).await;
    let sweeper = scene.sweeper(&gc_config(&scene.rig, 5, LEASE, 60 * SECOND), None);
    let janitor: &dyn Janitor = &sweeper;

    // Act
    let stats = janitor
        .run_cycle(after(START, SECOND))
        .await
        .expect("cycle");

    // Assert
    assert_eq!(janitor.name(), "gc_sweeper");
    assert_eq!(stats.processed, 12);
    drop(sweeper);
    scene.finish().await;
}
