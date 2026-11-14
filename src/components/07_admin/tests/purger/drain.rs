//! IT-07-9 (AZ-3820 AC-1, AZ-3825 AC-1, AC-27, AC-30): the purger drains the real hooks of 06, 02, 03, 08, 05 and
//! 07a in order with 04's GC between cycles, finalizes once and last, and leaves no row (jobs and derived rows
//! included) or blob file of the account while other accounts keep theirs; every derived blob is queued
//! `account_deleted` and due at once before 04's GC removes its file. The self-deadlock check: a hook that inserts a row referencing the account on its own
//! connection while the purger holds its claim finishes (`FOR NO KEY UPDATE` leaves `FOR KEY SHARE` free).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use contracts::{AccountLifecycleHook, EraseProgress, Janitor, LifecycleError, async_trait};
use sc_clock::{Clock, SystemClock};
use sc_db::{Db, DbTx, Scope};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::purge_support::probes::{
    MAX_CYCLES, deleted_audits, drain, files_left, rows_of, state, total,
};
use crate::purge_support::recording::{
    Call, Log, RecordedAccounts, failed_calls, order_breach, record_hooks,
};
use crate::purge_support::seed::{self, TREE_SMALL};
use crate::purge_support::world::{HOOK_NAMES, MEDIA_POSITION, PurgeWorld};

const INSERT_WAIT: Duration = Duration::from_secs(5);

const INSERT_RECEIPT: &str = "INSERT INTO commit_receipts (account_id, commit_id, request_hash, response, created_at) \
     VALUES ($1, gen_random_uuid(), decode(repeat('07', 32), 'hex'), '[]'::jsonb, \
     timestamptz '2026-11-07 12:00:00+00')";

fn system_clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock::new())
}

/// Blobs of the account whose owner row (revision, upload session or derived row) is gone but which are not
/// queued `account_deleted` and due by now.
async fn unqueued_orphans(conn: &mut PgConnection, account: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM blobs b WHERE b.account_id = $1 \
         AND NOT EXISTS (SELECT 1 FROM revisions r WHERE r.blob_id = b.id) \
         AND NOT EXISTS (SELECT 1 FROM upload_sessions s WHERE s.blob_id = b.id) \
         AND NOT EXISTS (SELECT 1 FROM derived d WHERE d.blob_id = b.id) \
         AND NOT EXISTS (SELECT 1 FROM blob_gc g WHERE g.blob_id = b.id \
         AND g.reason = 'account_deleted' AND g.due_at <= clock_timestamp())",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count unqueued blobs")
}

/// Of `blobs`, those queued `account_deleted` and due by now.
async fn queued_now(conn: &mut PgConnection, account: Uuid, blobs: &[Uuid]) -> BTreeSet<Uuid> {
    let queued: Vec<Uuid> = sqlx::query_scalar(
        "SELECT blob_id FROM blob_gc WHERE account_id = $1 AND blob_id = ANY($2) \
         AND reason = 'account_deleted' AND due_at <= clock_timestamp()",
    )
    .bind(account)
    .bind(blobs)
    .fetch_all(conn)
    .await
    .expect("read queued blobs");
    queued.into_iter().collect()
}

/// (jobs, derived rows) of `account`.
async fn media_rows(conn: &mut PgConnection, account: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM jobs WHERE account_id = $1), \
         (SELECT count(*) FROM derived WHERE account_id = $1)",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count media rows")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_9_purger_drains_every_hook_in_order_and_finalizes_last() {
    // Arrange
    let world = PurgeWorld::new(system_clock()).await;
    let wiring = world.wiring(world.api.clone()).await;
    let target = seed::account(&world, TREE_SMALL).await;
    let other = seed::account(&world, seed::files(30)).await;
    let mut conn = world.fixture().await;
    let media_before = media_rows(&mut conn, target.account).await;
    let other_before = rows_of(&mut conn, other.account).await;
    world.request_deletion(target.account).await;
    let log = Log::default();
    let hooks = record_hooks(wiring.hooks.clone(), &log, None);
    let accounts = RecordedAccounts::wrap(wiring.accounts.clone(), &log, false);
    let purger = world.purger(accounts, hooks, world.api.clone());
    let sweeper = world.sweeper(&wiring);

    // Act
    let mut cycles = 0;
    let mut worst_orphans = 0;
    let mut derived_queued = BTreeSet::new();
    while state(&mut conn, target.account).await.is_some() {
        assert!(
            cycles < MAX_CYCLES,
            "the account is not gone after {MAX_CYCLES} cycles"
        );
        cycles += 1;
        purger
            .run_cycle(world.clock.now_utc())
            .await
            .expect("purger cycle");
        worst_orphans = worst_orphans.max(unqueued_orphans(&mut conn, target.account).await);
        derived_queued.extend(queued_now(&mut conn, target.account, &target.derived).await);
        sweeper
            .run_cycle(world.clock.now_utc())
            .await
            .expect("gc cycle");
    }

    // Assert
    assert!(cycles > 1);
    assert_eq!(media_before, (30, 60));
    let calls = log.calls();
    assert_eq!(
        order_breach(&calls, HOOK_NAMES.len(), 50),
        None,
        "{} calls",
        calls.len()
    );
    assert_eq!(failed_calls(&calls), 0);
    assert!(
        calls.iter().any(|call| matches!(
            call,
            Call::Hook { position: MEDIA_POSITION, remaining: Some(left), .. } if *left > 0
        )),
        "08's hook never erased a derived row"
    );
    assert_eq!(worst_orphans, 0, "freed blobs are queued Due::Now");
    assert_eq!(
        derived_queued,
        target.derived.iter().copied().collect::<BTreeSet<_>>(),
        "every derived blob is queued account_deleted, due now"
    );
    let left = rows_of(&mut conn, target.account).await;
    assert_eq!(total(&left), 0, "{left:?}");
    assert_eq!((left["jobs"], left["derived"]), (0, 0));
    assert_eq!(files_left(&world, &target.derived), 0);
    assert_eq!(files_left(&world, &target.blobs), 0);
    assert_eq!(deleted_audits(&mut conn, target.account).await, 1);
    assert_eq!(rows_of(&mut conn, other.account).await, other_before);
    assert_eq!(files_left(&world, &other.blobs), other.blobs.len());
    conn.close().await.expect("close");
    world.finish().await;
}

/// Before delegating its first call, inserts a commit receipt of the account on its own connection, as a hook
/// that writes a row referencing the account would; 03's erase removes the receipt later.
struct InsertsReference {
    inner: Arc<dyn AccountLifecycleHook>,
    db: Db,
    pending: AtomicBool,
    inserted: AtomicBool,
}

#[async_trait]
impl AccountLifecycleHook for InsertsReference {
    async fn initialize(&self, tx: &mut DbTx, account: Uuid) -> Result<(), LifecycleError> {
        self.inner.initialize(tx, account).await
    }

    async fn erase_batch(
        &self,
        account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        if self.pending.swap(false, Ordering::SeqCst) {
            let insert = self.db.run(Scope::Account(account), async |tx| {
                sqlx::query(INSERT_RECEIPT)
                    .bind(account)
                    .execute(tx.conn())
                    .await
                    .map(drop)
            });
            let done = tokio::time::timeout(INSERT_WAIT, insert).await;
            self.inserted
                .store(matches!(done, Ok(Ok(()))), Ordering::SeqCst);
        }
        self.inner.erase_batch(account, limit).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn it_07_9_a_hook_inserting_a_row_that_references_the_account_does_not_wait_for_the_claim() {
    // Arrange
    let world = PurgeWorld::new(system_clock()).await;
    let wiring = world.wiring(world.api.clone()).await;
    let target = seed::account(&world, seed::files(20)).await;
    world.request_deletion(target.account).await;
    let mut hooks = wiring.hooks.clone();
    let first = Arc::new(InsertsReference {
        inner: hooks[0].clone(),
        db: world.new_pool().await,
        pending: AtomicBool::new(true),
        inserted: AtomicBool::new(false),
    });
    hooks[0] = first.clone();
    let purger = world.purger(wiring.accounts.clone(), hooks, world.api.clone());
    let sweeper = world.sweeper(&wiring);

    // Act
    drain(&world, &purger, &sweeper, target.account).await;

    // Assert
    assert!(
        first.inserted.load(Ordering::SeqCst),
        "the insert waited for the claim"
    );
    let mut conn = world.fixture().await;
    assert_eq!(total(&rows_of(&mut conn, target.account).await), 0);
    assert_eq!(deleted_audits(&mut conn, target.account).await, 1);
    conn.close().await.expect("close");
    world.finish().await;
}

#[tokio::test]
async fn the_probe_insert_does_wait_for_a_row_held_for_update() {
    // Arrange
    let world = PurgeWorld::new(system_clock()).await;
    let target = seed::account(&world, seed::files(1)).await;
    let mut holder = world.fixture().await;
    let mut held = holder.begin().await.expect("begin");
    sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(target.account)
        .execute(&mut *held)
        .await
        .expect("hold the row");

    // Act
    let insert = world.api.run(Scope::Account(target.account), async |tx| {
        sqlx::query(INSERT_RECEIPT)
            .bind(target.account)
            .execute(tx.conn())
            .await
            .map(drop)
    });
    let waited = tokio::time::timeout(Duration::from_millis(500), insert).await;

    // Assert
    assert!(waited.is_err(), "the key-share check did not wait");
    held.rollback().await.expect("release");
    holder.close().await.expect("close");
    assert_eq!(
        state(&mut world.fixture().await, target.account)
            .await
            .as_deref(),
        Some("active")
    );
    world.finish().await;
}
