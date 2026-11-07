use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{BlobCleanupError, BlobOwnerCleanup, Due, GcQueue, GcReason, async_trait};
use sc_clock::{CancellationToken, ManualClock, Timestamp};
use sc_db::{DbTx, Scope};
use sqlx::PgConnection;
use sunnycloud_blobstore::{
    BlobRegistry, BlobstoreConfig, GcHook, GcHookPoint, GcHookStop, GcStats, GcSweeper, blob_path,
};
use tokio::sync::Barrier;
use uuid::Uuid;

use crate::rows::{Rig, START, account, config};

pub const RETENTION: Duration = Duration::from_secs(7 * 86_400);

/// The instant everything queued at the start of a test with `AfterRetention` becomes due.
pub fn due_time() -> Timestamp {
    START.checked_add(RETENTION).expect("due time")
}

pub fn gc_config(rig: &Rig, batch: u64, lease: Duration, budget: Duration) -> BlobstoreConfig {
    BlobstoreConfig {
        gc_batch: batch,
        gc_claim_lease: lease,
        gc_cycle_budget: budget,
        ..config(&rig.root, RETENTION)
    }
}

pub struct Scene {
    pub rig: Rig,
    pub account: Uuid,
    pub cleanup: Arc<Recording>,
}

impl Scene {
    pub async fn new() -> Scene {
        let rig = Rig::new().await;
        let account = account(&rig).await;
        Scene {
            rig,
            account,
            cleanup: Arc::new(Recording::default()),
        }
    }

    /// `count` committed rows, each with a real file, queued with `due`.
    pub async fn queue(&self, count: usize, due: Due) -> Vec<Uuid> {
        let ids: Vec<Uuid> = (0..count).map(|_| Uuid::new_v4()).collect();
        let mut conn = self.rig.owner().await;
        sqlx::query(
            "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
             SELECT id, $2, 'committed', 10, 'k'::bytea, 3, 'epoch'::timestamptz FROM unnest($1::uuid[]) AS t(id)",
        )
        .bind(&ids)
        .bind(self.account)
        .execute(&mut conn)
        .await
        .expect("insert blobs");
        sqlx::Connection::close(conn).await.expect("close");
        for &id in &ids {
            self.rig.put_file(id);
        }
        self.schedule(&ids, due).await;
        ids
    }

    pub async fn schedule(&self, ids: &[Uuid], due: Due) {
        self.rig
            .api
            .run(Scope::Account(self.account), async |tx| {
                for &id in ids {
                    self.rig
                        .queue
                        .schedule_in_tx(tx, id, GcReason::Deleted, due)
                        .await?;
                }
                Ok::<_, contracts::GcQueueError>(())
            })
            .await
            .expect("schedule");
    }

    pub fn sweeper(&self, config: &BlobstoreConfig, hook: Option<Arc<dyn GcHook>>) -> GcSweeper {
        self.sweeper_with(config, hook, CancellationToken::new())
    }

    pub fn sweeper_with(
        &self,
        config: &BlobstoreConfig,
        hook: Option<Arc<dyn GcHook>>,
        shutdown: CancellationToken,
    ) -> GcSweeper {
        let registry =
            Arc::new(BlobRegistry::new(config, self.rig.clock.clone()).expect("registry"));
        let cleanup: Arc<dyn BlobOwnerCleanup> = self.cleanup.clone();
        let sweeper = GcSweeper::new(
            config,
            self.rig.api.clone(),
            self.rig.clock.clone(),
            registry,
            vec![cleanup],
            shutdown,
        )
        .expect("sweeper");
        match hook {
            Some(hook) => sweeper.with_hook(hook),
            None => sweeper,
        }
    }

    pub async fn run(sweeper: &GcSweeper, now: Timestamp) -> GcStats {
        sweeper.run_cycle(now).await.expect("cycle")
    }

    pub async fn rows(&self) -> i64 {
        self.count("SELECT count(*) FROM blobs").await
    }

    pub async fn queue_rows(&self) -> i64 {
        self.count("SELECT count(*) FROM blob_gc").await
    }

    async fn count(&self, sql: &'static str) -> i64 {
        let mut conn = self.rig.owner().await;
        let count = sqlx::query_scalar(sql)
            .fetch_one(&mut conn)
            .await
            .expect("count");
        sqlx::Connection::close(conn).await.expect("close");
        count
    }

    pub fn file(&self, id: Uuid) -> PathBuf {
        blob_path(&self.rig.root, id)
    }

    pub fn has_file(&self, id: Uuid) -> bool {
        self.file(id).exists()
    }

    pub fn has_parked_file(&self, id: Uuid) -> bool {
        let mut name = self.file(id).into_os_string();
        name.push(".gc");
        PathBuf::from(name).exists()
    }

    pub async fn row_state(&self, id: Uuid) -> Option<String> {
        self.rig.row(id).await.map(|(state, _)| state)
    }

    pub async fn finish(self) {
        let Scene { rig, cleanup, .. } = self;
        drop(cleanup);
        rig.finish().await;
    }
}

pub fn after(now: Timestamp, by: Duration) -> Timestamp {
    now.checked_add(by).expect("later")
}

/// Records every call of the cleanup, whether the blob row was still there, and can refuse chosen blobs.
#[derive(Default)]
pub struct Recording {
    seen: Mutex<HashMap<Uuid, (u32, bool)>>,
    refuse: Mutex<HashSet<Uuid>>,
}

impl Recording {
    pub fn refuse(&self, id: Uuid) {
        self.refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id);
    }

    pub fn allow_all(&self) {
        self.refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// Calls per blob, and whether the row of the blob existed in the transaction at each call.
    pub fn calls(&self, id: Uuid) -> Option<(u32, bool)> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .copied()
    }

    pub fn total(&self) -> usize {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[async_trait]
impl BlobOwnerCleanup for Recording {
    async fn remove_owner_rows(
        &self,
        tx: &mut DbTx,
        blob_id: Uuid,
    ) -> Result<(), BlobCleanupError> {
        let present: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM blobs WHERE id = $1)")
            .bind(blob_id)
            .fetch_one(tx.conn())
            .await
            .map_err(|error| BlobCleanupError::Unexpected(Box::new(error)))?;
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = seen.entry(blob_id).or_insert((0, true));
        entry.0 += 1;
        entry.1 &= present;
        let refused = self
            .refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&blob_id);
        if refused {
            return Err(BlobCleanupError::Unexpected("refused by the test".into()));
        }
        Ok(())
    }
}

/// Counts the points of every batch. It can move the manual clock at each settle (the cost of a batch) and end
/// the next batch before its settling transaction, as a process death would.
#[derive(Default)]
pub struct Counts {
    pub claimed: AtomicUsize,
    pub parked: AtomicUsize,
    pub settles: AtomicUsize,
    tick: Option<(Arc<ManualClock>, Duration)>,
    stop_once: AtomicBool,
}

impl Counts {
    pub fn new() -> Arc<Counts> {
        Arc::new(Counts::default())
    }

    pub fn ticking(clock: Arc<ManualClock>, by: Duration) -> Arc<Counts> {
        Arc::new(Counts {
            tick: Some((clock, by)),
            ..Counts::default()
        })
    }

    pub fn stopping_one_batch() -> Arc<Counts> {
        Arc::new(Counts {
            stop_once: AtomicBool::new(true),
            ..Counts::default()
        })
    }

    pub fn claims(&self) -> usize {
        self.claimed.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl GcHook for Counts {
    async fn at(&self, point: GcHookPoint) -> Result<(), GcHookStop> {
        match point {
            GcHookPoint::Claimed => {
                self.claimed.fetch_add(1, Ordering::SeqCst);
            }
            GcHookPoint::Parked => {
                self.parked.fetch_add(1, Ordering::SeqCst);
            }
            GcHookPoint::BeforeSettle => {
                self.settles.fetch_add(1, Ordering::SeqCst);
                if let Some((clock, by)) = &self.tick {
                    clock.tick(*by);
                }
                if self.stop_once.swap(false, Ordering::SeqCst) {
                    return Err(GcHookStop);
                }
            }
        }
        Ok(())
    }
}

/// Holds the first batch at each of `points`, in order, until the test lets it go: for every point the test waits
/// on the barrier twice, once when the batch has arrived and once to release it.
pub struct Pause {
    points: Mutex<Vec<GcHookPoint>>,
    barrier: Barrier,
}

impl Pause {
    pub fn at(points: &[GcHookPoint]) -> Arc<Pause> {
        Arc::new(Pause {
            points: Mutex::new(points.to_vec()),
            barrier: Barrier::new(2),
        })
    }

    pub async fn arrived(&self) {
        self.barrier.wait().await;
    }

    pub async fn release(&self) {
        self.barrier.wait().await;
    }
}

#[async_trait]
impl GcHook for Pause {
    async fn at(&self, point: GcHookPoint) -> Result<(), GcHookStop> {
        let held = {
            let mut points = self.points.lock().unwrap_or_else(PoisonError::into_inner);
            let next = points.first() == Some(&point);
            if next {
                points.remove(0);
            }
            next
        };
        if held {
            self.barrier.wait().await;
            self.barrier.wait().await;
        }
        Ok(())
    }
}

/// Counts the sessions of the test database that sit idle inside a transaction, each time a blob file has just
/// been moved: the sweeper must show none.
pub struct IdleInTransaction {
    conn: tokio::sync::Mutex<PgConnection>,
    pub seen: Mutex<Vec<i64>>,
}

pub const IDLE_IN_TRANSACTION: &str = "SELECT count(*) FROM pg_stat_activity \
     WHERE datname = current_database() AND state LIKE 'idle in transaction%'";

impl IdleInTransaction {
    pub async fn new(rig: &Rig) -> Arc<IdleInTransaction> {
        Arc::new(IdleInTransaction {
            conn: tokio::sync::Mutex::new(rig.db.admin().await),
            seen: Mutex::default(),
        })
    }

    pub fn counts(&self) -> Vec<i64> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub async fn close(self: Arc<Self>) {
        let hook = Arc::into_inner(self).expect("hook still shared");
        sqlx::Connection::close(hook.conn.into_inner())
            .await
            .expect("close probe");
    }
}

#[async_trait]
impl GcHook for IdleInTransaction {
    async fn at(&self, point: GcHookPoint) -> Result<(), GcHookStop> {
        if point == GcHookPoint::Parked {
            let mut conn = self.conn.lock().await;
            let idle: i64 = sqlx::query_scalar(IDLE_IN_TRANSACTION)
                .fetch_one(&mut *conn)
                .await
                .expect("probe pg_stat_activity");
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(idle);
        }
        Ok(())
    }
}
