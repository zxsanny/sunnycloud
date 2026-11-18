//! IT-07-11 (AZ-3820 AC-3): with batch 50 and a 300 ms cycle budget, a 5,000-file account needs several cycles,
//! no call passes a limit above 50, no transaction of the API role stays open for 1 s (sampled every 50 ms), and
//! the end state equals IT-07-9's.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sc_clock::{Clock, SystemClock};
use sqlx::Connection;

use crate::purge_support::probes::{
    deleted_audits, drain, files_left, longest_transaction, rows_of, total,
};
use crate::purge_support::recording::{
    Log, RecordedAccounts, failed_calls, order_breach, record_hooks,
};
use crate::purge_support::seed;
use crate::purge_support::world::{HOOK_NAMES, PurgeWorld};

const SAMPLE_EVERY: Duration = Duration::from_millis(50);
const LONGEST_ALLOWED: Duration = Duration::from_secs(1);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_11_cycles_stop_at_the_budget_with_batches_of_50_and_short_transactions() {
    // Arrange
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let world = PurgeWorld::new(clock).await;
    let wiring = world.wiring(world.api.clone()).await;
    let target = seed::account(&world, seed::files(5000)).await;
    world.request_deletion(target.account).await;
    let log = Log::default();
    let hooks = record_hooks(wiring.hooks.clone(), &log, None);
    let accounts = RecordedAccounts::wrap(wiring.accounts.clone(), &log, false);
    let purger = world.purger(accounts, hooks, world.api.clone());
    let sweeper = world.sweeper(&wiring);
    let mut sampler_conn = world.db.admin().await;
    let done = Arc::new(AtomicBool::new(false));
    let sampling = done.clone();
    let sampler = tokio::spawn(async move {
        let mut longest = Duration::ZERO;
        let mut samples = 0u32;
        while !sampling.load(Ordering::SeqCst) {
            longest = longest.max(longest_transaction(&mut sampler_conn).await);
            samples += 1;
            tokio::time::sleep(SAMPLE_EVERY).await;
        }
        sampler_conn.close().await.expect("close");
        (longest, samples)
    });

    // Act
    let cycles = drain(&world, &purger, &sweeper, target.account).await;
    done.store(true, Ordering::SeqCst);
    let (longest, samples) = sampler.await.expect("sampler");

    // Assert
    assert!(cycles > 1, "{cycles} cycles");
    assert!(samples > 1);
    assert!(
        longest < LONGEST_ALLOWED,
        "a transaction stayed open {longest:?}"
    );
    assert_eq!(order_breach(&log.calls(), HOOK_NAMES.len(), 50), None);
    assert_eq!(failed_calls(&log.calls()), 0);
    let mut conn = world.fixture().await;
    let left = rows_of(&mut conn, target.account).await;
    assert_eq!(total(&left), 0, "{left:?}");
    assert_eq!(files_left(&world, &target.blobs), 0);
    assert_eq!(deleted_audits(&mut conn, target.account).await, 1);
    conn.close().await.expect("close");
    world.finish().await;
}
