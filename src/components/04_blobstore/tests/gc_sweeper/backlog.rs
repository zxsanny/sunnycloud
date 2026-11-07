//! AC-1, AC-2, AC-3 and AC-5: what a cycle removes and when it stops.

use std::time::Duration;

use contracts::Due;
use sc_clock::Timestamp;
use sunnycloud_blobstore::GcStats;

use crate::rows::START;
use crate::support::{Counts, Scene, after, due_time, gc_config};

const SECOND: Duration = Duration::from_secs(1);

#[tokio::test]
async fn nothing_is_removed_before_the_due_time_and_everything_at_it() {
    // Arrange
    let scene = Scene::new().await;
    let ids = scene.queue(270, Due::AfterRetention).await;
    let sweeper = scene.sweeper(&gc_config(&scene.rig, 100, 60 * SECOND, 60 * SECOND), None);
    let just_before = Timestamp::from_unix_micros(due_time().as_unix_micros() - 1_000);

    // Act
    let early = Scene::run(&sweeper, just_before).await;
    let early_rows = scene.rows().await;
    let early_files = ids.iter().filter(|id| scene.has_file(**id)).count();
    let on_time = Scene::run(&sweeper, due_time()).await;

    // Assert
    assert_eq!(early, GcStats::default());
    assert_eq!((early_rows, early_files), (270, 270));
    assert_eq!(scene.cleanup.total(), 270);
    assert_eq!(
        on_time,
        GcStats {
            removed: 270,
            remaining_due: 0,
            budget_spent: false
        }
    );
    assert_eq!((scene.rows().await, scene.queue_rows().await), (0, 0));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn a_backlog_drains_in_one_cycle_in_batches_and_future_blobs_stay() {
    // Arrange
    let scene = Scene::new().await;
    let due = scene.queue(250, Due::Now).await;
    let future = scene.queue(20, Due::AfterRetention).await;
    let counts = Counts::new();
    let config = gc_config(&scene.rig, 10, 60 * SECOND, 5 * SECOND);
    let sweeper = scene.sweeper(&config, Some(counts.clone()));

    // Act
    let stats = Scene::run(&sweeper, after(START, SECOND)).await;

    // Assert
    assert_eq!(
        stats,
        GcStats {
            removed: 250,
            remaining_due: 0,
            budget_spent: false
        }
    );
    assert_eq!(counts.claims(), 25);
    assert!(
        due.iter()
            .all(|id| !scene.has_file(*id) && !scene.has_parked_file(*id))
    );
    assert!(future.iter().all(|id| scene.has_file(*id)));
    assert_eq!((scene.rows().await, scene.queue_rows().await), (20, 20));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn an_empty_queue_ends_the_loop_with_zero_stats() {
    // Arrange
    let scene = Scene::new().await;
    let counts = Counts::new();
    let sweeper = scene.sweeper(
        &gc_config(&scene.rig, 10, 60 * SECOND, 5 * SECOND),
        Some(counts.clone()),
    );

    // Act
    let stats = Scene::run(&sweeper, due_time()).await;

    // Assert
    assert_eq!(stats, GcStats::default());
    assert_eq!(counts.claims(), 0);
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn the_budget_ends_a_cycle_and_the_next_cycles_finish_the_rest() {
    // Arrange
    let scene = Scene::new().await;
    scene.queue(100, Due::Now).await;
    let counts = Counts::ticking(scene.rig.clock.clone(), 2 * SECOND);
    let config = gc_config(&scene.rig, 10, 60 * SECOND, 5 * SECOND);
    let sweeper = scene.sweeper(&config, Some(counts));
    let now = after(START, SECOND);

    // Act
    let first = Scene::run(&sweeper, now).await;
    let mut removed = first.removed;
    let mut last = first;
    while last.remaining_due > 0 {
        last = Scene::run(&sweeper, now).await;
        removed += last.removed;
    }

    // Assert
    assert_eq!(
        first,
        GcStats {
            removed: 30,
            remaining_due: 70,
            budget_spent: true
        }
    );
    assert_eq!(removed, 100);
    assert!(!last.budget_spent || last.remaining_due == 0);
    assert_eq!(scene.rows().await, 0);
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn cleanup_runs_once_per_blob_inside_the_transaction_before_its_row_goes() {
    // Arrange
    let scene = Scene::new().await;
    let ids = scene.queue(250, Due::Now).await;
    let sweeper = scene.sweeper(&gc_config(&scene.rig, 10, 60 * SECOND, 60 * SECOND), None);

    // Act
    let stats = Scene::run(&sweeper, after(START, SECOND)).await;

    // Assert
    assert_eq!(stats.removed, 250);
    assert_eq!(scene.cleanup.total(), 250);
    for id in ids {
        assert_eq!(scene.cleanup.calls(id), Some((1, true)), "blob {id}");
    }
    assert_eq!(scene.rows().await, 0);
    drop(sweeper);
    scene.finish().await;
}
