//! IT-07-10 (AZ-3820 AC-2, AZ-3825 AC-2, NFT-RES-30): a 500-file account with its jobs and derived rows; one round
//! per position where the first call fails (each hook of 06, 02, 03, 08, 05, 07a, then `finalize_deletion`), and one
//! round where the cycle is dropped mid-step
//! and a fresh purger instance on a fresh pool takes over. Each failure is one ERROR line with the account id and
//! the position and no row data; the purge resumes without further errors; the account stays `deleting` until the
//! finalize; the end is clean with one `account_deleted`.

use std::sync::Arc;

use contracts::Janitor;
use sc_clock::{Clock, SystemClock};
use sqlx::Connection;
use tokio::sync::Notify;

use crate::capture;
use crate::purge_support::probes::{
    MAX_CYCLES, deleted_audits, drain, files_left, rows_of, state, total,
};
use crate::purge_support::recording::{Fault, Log, RecordedAccounts, failed_calls, record_hooks};
use crate::purge_support::seed::{self, NAME_MARKER};
use crate::purge_support::world::{HOOK_NAMES, MEDIA_POSITION, PurgeWorld};

const FILES: usize = 500;

/// One round with the first call at `position` failing; `HOOK_NAMES.len()` is the finalize.
async fn failing_round(position: usize, expected: &str) {
    // Arrange
    capture::install();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let world = PurgeWorld::new(clock).await;
    let wiring = world.wiring(world.api.clone()).await;
    let target = seed::account(&world, seed::files(FILES)).await;
    world.request_deletion(target.account).await;
    let log = Log::default();
    let fault = (position < HOOK_NAMES.len()).then_some((position, Fault::FailNext));
    let hooks = record_hooks(wiring.hooks.clone(), &log, fault);
    let accounts =
        RecordedAccounts::wrap(wiring.accounts.clone(), &log, position == HOOK_NAMES.len());
    let purger = world.purger(accounts, hooks, world.api.clone());
    let sweeper = world.sweeper(&wiring);
    let mut conn = world.fixture().await;

    // Act
    let mut state_after_failure = None;
    let mut gone = false;
    for _ in 0..MAX_CYCLES {
        purger
            .run_cycle(world.clock.now_utc())
            .await
            .expect("purger cycle");
        if state_after_failure.is_none() && failed_calls(&log.calls()) > 0 {
            state_after_failure = Some(state(&mut conn, target.account).await);
        }
        if state(&mut conn, target.account).await.is_none() {
            gone = true;
            break;
        }
        sweeper
            .run_cycle(world.clock.now_utc())
            .await
            .expect("gc cycle");
    }

    // Assert
    assert!(gone, "the account is not gone after {MAX_CYCLES} cycles");
    assert_eq!(
        state_after_failure,
        Some(Some("deleting".to_owned())),
        "the failure did not fire or the account left deleting early"
    );
    let left = rows_of(&mut conn, target.account).await;
    assert_eq!(total(&left), 0, "{left:?}");
    assert_eq!(files_left(&world, &target.blobs), 0);
    assert_eq!(deleted_audits(&mut conn, target.account).await, 1);
    let lines = capture::of_account(target.account);
    let errors = capture::at_level(&lines, "error");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(errors[0].contains(expected), "{}", errors[0]);
    assert!(!errors[0].contains(NAME_MARKER), "{}", errors[0]);
    conn.close().await.expect("close");
    world.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_failure_at_the_share_hook_is_logged_and_the_purge_resumes() {
    failing_round(0, "hook position 0").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_failure_at_the_tree_hook_is_logged_and_the_purge_resumes() {
    failing_round(1, "hook position 1").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_failure_at_the_transfer_hook_is_logged_and_the_purge_resumes() {
    failing_round(2, "hook position 2").await;
}

/// AZ-3825 AC-2: 08 is the fourth hook of the v1.0 list; the log names its 0-based index, 3.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_failure_at_the_media_hook_is_logged_and_the_purge_resumes() {
    failing_round(MEDIA_POSITION, "hook position 3").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_failure_at_the_feed_hook_is_logged_and_the_purge_resumes() {
    failing_round(4, "hook position 4").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_failure_at_the_quota_hook_is_logged_and_the_purge_resumes() {
    failing_round(5, "hook position 5").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_failure_at_finalize_is_logged_and_the_purge_resumes() {
    failing_round(6, "finalize (position 6)").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_10_a_cycle_dropped_mid_step_is_finished_by_a_fresh_purger() {
    // Arrange
    capture::install();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let world = PurgeWorld::new(clock).await;
    let wiring = world.wiring(world.api.clone()).await;
    let target = seed::account(&world, seed::files(FILES)).await;
    world.request_deletion(target.account).await;
    let entered = Arc::new(Notify::new());
    let log = Log::default();
    let hooks = record_hooks(
        wiring.hooks.clone(),
        &log,
        Some((1, Fault::HangAfterNext(entered.clone()))),
    );
    let stuck = world.purger(wiring.accounts.clone(), hooks, world.api.clone());

    // Act
    {
        let cycle = stuck.run_cycle(world.clock.now_utc());
        tokio::pin!(cycle);
        tokio::select! {
            _ = &mut cycle => panic!("the cycle ended before the hang"),
            () = entered.notified() => {}
        }
    }
    drop(stuck);
    let mut conn = world.fixture().await;
    let state_after_drop = state(&mut conn, target.account).await;
    let fresh = world.wiring(world.new_pool().await).await;
    let purger = world.purger(
        fresh.accounts.clone(),
        fresh.hooks.clone(),
        fresh.db.clone(),
    );
    drain(&world, &purger, &world.sweeper(&fresh), target.account).await;

    // Assert
    assert_eq!(state_after_drop.as_deref(), Some("deleting"));
    let left = rows_of(&mut conn, target.account).await;
    assert_eq!(total(&left), 0, "{left:?}");
    assert_eq!(files_left(&world, &target.blobs), 0);
    assert_eq!(deleted_audits(&mut conn, target.account).await, 1);
    let lines = capture::of_account(target.account);
    assert_eq!(capture::at_level(&lines, "error").len(), 0, "{lines:#?}");
    conn.close().await.expect("close");
    world.finish().await;
}
