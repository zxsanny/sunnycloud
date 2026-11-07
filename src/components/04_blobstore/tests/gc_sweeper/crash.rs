//! AC-6 and AC-7: a batch that dies between the file step and the settling transaction, and no transaction
//! across the file step.

use std::time::Duration;

use contracts::Due;
use sc_db::Scope;
use uuid::Uuid;

use crate::rows::START;
use crate::support::{Counts, IDLE_IN_TRANSACTION, IdleInTransaction, Scene, after, gc_config};

const SECOND: Duration = Duration::from_secs(1);
const LEASE: Duration = Duration::from_secs(60);

fn invariant_holds(scene: &Scene, id: Uuid, row_exists: bool) -> bool {
    scene.has_file(id) == row_exists && !scene.has_parked_file(id)
}

#[tokio::test]
async fn a_batch_that_dies_after_the_file_step_is_finished_by_the_cycle_after_the_lease() {
    // Arrange
    let scene = Scene::new().await;
    let ids = scene.queue(30, Due::Now).await;
    let counts = Counts::stopping_one_batch();
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper(&config, Some(counts));
    let now = after(START, SECOND);

    // Act
    let first = Scene::run(&sweeper, now).await;
    let stuck: Vec<Uuid> = {
        let mut stuck = Vec::new();
        for &id in &ids {
            if scene.row_state(id).await.is_some() {
                stuck.push(id);
            }
        }
        stuck
    };
    let stuck_files_parked = stuck
        .iter()
        .all(|id| !scene.has_file(*id) && scene.has_parked_file(*id));
    let before_lease = Scene::run(&sweeper, after(now, LEASE - SECOND)).await;
    let after_lease = Scene::run(&sweeper, after(now, LEASE + 2 * SECOND)).await;

    // Assert
    assert_eq!((first.removed, first.remaining_due), (20, 0));
    assert_eq!(stuck.len(), 10);
    assert!(
        stuck_files_parked,
        "the interrupted blobs have a row and no file at its path"
    );
    assert_eq!(before_lease.removed, 0, "claimed rows wait for the lease");
    assert_eq!(after_lease.removed, 10);
    for id in ids {
        assert!(scene.row_state(id).await.is_none());
        assert!(invariant_holds(&scene, id, false), "blob {id}");
    }
    assert_eq!((scene.rows().await, scene.queue_rows().await), (0, 0));
    drop(sweeper);
    scene.finish().await;
}

#[tokio::test]
async fn no_session_idles_in_a_transaction_while_the_sweeper_moves_files() {
    // Arrange
    let scene = Scene::new().await;
    scene.queue(25, Due::Now).await;
    let probe = IdleInTransaction::new(&scene.rig).await;
    let config = gc_config(&scene.rig, 10, LEASE, 60 * SECOND);
    let sweeper = scene.sweeper(&config, Some(probe.clone()));

    // Act
    let stats = Scene::run(&sweeper, after(START, SECOND)).await;

    // Assert
    assert_eq!(stats.removed, 25);
    assert_eq!(probe.counts(), vec![0; 25]);
    drop(sweeper);
    probe.close().await;
    scene.finish().await;
}

#[tokio::test]
async fn the_idle_in_transaction_probe_sees_a_transaction_that_is_held_open() {
    // Arrange
    let scene = Scene::new().await;
    let held = scene
        .rig
        .api
        .run(Scope::Account(scene.account), async |tx| {
            let mut other = scene.rig.db.admin().await;
            let idle: i64 = sqlx::query_scalar(IDLE_IN_TRANSACTION)
                .fetch_one(&mut other)
                .await
                .expect("probe");
            sqlx::Connection::close(other).await.expect("close");
            sqlx::query("SELECT 1")
                .execute(tx.conn())
                .await
                .map(|_| idle)
        })
        .await
        .expect("held");

    // Assert
    assert_eq!(
        held, 1,
        "the probe must count a session inside a transaction"
    );
    scene.finish().await;
}
