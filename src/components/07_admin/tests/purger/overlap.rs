//! IT-07-12 (AZ-3820 AC-4): two purger instances, each with its own pool and hooks, loop over one account
//! together with 04's GC; the end state is clean, no cycle returns an error and one `account_deleted` exists.

use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::Janitor;
use sc_clock::{Clock, SystemClock};
use sc_testdb::MIGRATOR;
use sqlx::Connection;
use sunnycloud_admin::DeletionPurger;
use uuid::Uuid;

use crate::purge_support::probes::{deleted_audits, files_left, rows_of, state, total};
use crate::purge_support::seed;
use crate::purge_support::world::PurgeWorld;

/// The pause between two cycles of one purger, standing in for the scheduler's interval.
const PAUSE: Duration = Duration::from_millis(20);
/// Far more than the purge needs; past it the purge is stuck.
const GIVE_UP: Duration = Duration::from_secs(180);

/// Cycles until the account is gone; returns the cycles that returned an error.
async fn purge_until_gone(world: &PurgeWorld, purger: &DeletionPurger, account: Uuid) -> usize {
    let mut conn = world.db.conn_as(MIGRATOR).await;
    let started = Instant::now();
    let mut failures = 0;
    while started.elapsed() < GIVE_UP {
        if purger.run_cycle(world.clock.now_utc()).await.is_err() {
            failures += 1;
        }
        if state(&mut conn, account).await.is_none() {
            conn.close().await.expect("close");
            return failures;
        }
        tokio::time::sleep(PAUSE).await;
    }
    panic!("the account is not gone after {GIVE_UP:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn it_07_12_two_purgers_on_separate_connections_finish_one_account_once() {
    // Arrange
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let world = PurgeWorld::new(clock).await;
    let first = world.wiring(world.new_pool().await).await;
    let second = world.wiring(world.new_pool().await).await;
    let target = seed::account(&world, seed::files(2000)).await;
    world.request_deletion(target.account).await;
    let one = world.purger(
        first.accounts.clone(),
        first.hooks.clone(),
        first.db.clone(),
    );
    let two = world.purger(
        second.accounts.clone(),
        second.hooks.clone(),
        second.db.clone(),
    );
    let sweeper = world.sweeper(&first);
    let gc = async {
        let mut conn = world.db.conn_as(MIGRATOR).await;
        while state(&mut conn, target.account).await.is_some() {
            sweeper
                .run_cycle(world.clock.now_utc())
                .await
                .expect("gc cycle");
            tokio::time::sleep(PAUSE).await;
        }
        conn.close().await.expect("close");
    };

    // Act
    let (failed_one, failed_two, ()) = tokio::join!(
        purge_until_gone(&world, &one, target.account),
        purge_until_gone(&world, &two, target.account),
        gc,
    );

    // Assert
    assert_eq!((failed_one, failed_two), (0, 0));
    let mut conn = world.fixture().await;
    let left = rows_of(&mut conn, target.account).await;
    assert_eq!(total(&left), 0, "{left:?}");
    assert_eq!(files_left(&world, &target.blobs), 0);
    assert_eq!(deleted_audits(&mut conn, target.account).await, 1);
    conn.close().await.expect("close");
    world.finish().await;
}
