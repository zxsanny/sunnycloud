//! The seeded ledger workload of 02 and 03 on one account: commit-shaped transactions (`reserve`, then
//! `release_reservation` and `adjust_used(+size)`) and delete-shaped ones (`adjust_used(-size)` of an earlier
//! commit), in waves whose transactions start together and each retried once on a deadlock. Kinds and sizes come
//! from the seed; a delete targets a commit that actually succeeded in an earlier wave.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use contracts::{QuotaLedger, QuotaLedgerError};
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use sqlx::{Connection, PgConnection};
use tokio::sync::Barrier;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::support::{GIB, MIB, ledger, quota_row};

pub const SEED: u64 = 20_261_103;
pub const WAVES: usize = 20;
pub const WAVE_SIZE: usize = 50;
pub const LIMIT: i64 = 200 * MIB;
pub const SAMPLE_EVERY: Duration = Duration::from_millis(5);
const MIN_SIZE: i64 = 1024;
const MAX_SIZE: i64 = 4 * MIB;
const DELETE_PERCENT: u64 = 40;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).expect("bound fits")).expect("fits")
    }

    fn size(&mut self) -> i64 {
        MIN_SIZE
            + i64::try_from(self.next() % u64::try_from(MAX_SIZE - MIN_SIZE + 1).expect("range"))
                .expect("fits")
    }
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Commit { size: i64 },
    CommitRolledBack { size: i64 },
    Delete { size: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Granted(i64),
    Refused,
    RolledBack,
    Deleted,
}

#[derive(Debug)]
enum Abort {
    Ledger(QuotaLedgerError),
    Db(DbError),
    Rollback,
}

impl From<QuotaLedgerError> for Abort {
    fn from(error: QuotaLedgerError) -> Self {
        Abort::Ledger(error)
    }
}

impl From<DbError> for Abort {
    fn from(error: DbError) -> Self {
        Abort::Db(error)
    }
}

/// What the harness knows after the run: its own ledger of every success, and every error that surfaced.
#[derive(Debug, Default)]
pub struct Report {
    pub ledger_sum: i64,
    pub granted: usize,
    pub refused: usize,
    pub deleted: usize,
    pub rolled_back: usize,
    pub retried: usize,
    pub errors: Vec<String>,
}

fn plan_wave(rng: &mut Rng, live: &mut Vec<i64>, rollback_percent: usize) -> Vec<Op> {
    let mut deletable: Vec<i64> = std::mem::take(live);
    let mut ops = Vec::with_capacity(WAVE_SIZE);
    for _ in 0..WAVE_SIZE {
        let delete = !deletable.is_empty()
            && rng.below(100) < usize::try_from(DELETE_PERCENT).expect("percent");
        if delete {
            let at = rng.below(deletable.len());
            ops.push(Op::Delete {
                size: deletable.swap_remove(at),
            });
        } else {
            let size = rng.size();
            ops.push(if rng.below(100) < rollback_percent {
                Op::CommitRolledBack { size }
            } else {
                Op::Commit { size }
            });
        }
    }
    *live = deletable;
    ops
}

async fn commit_shaped(tx: &mut DbTx, account: Uuid, size: i64) -> Result<Outcome, Abort> {
    let ledger = ledger(GIB);
    match ledger.reserve(tx, account, size).await {
        Ok(_) => {}
        Err(QuotaLedgerError::QuotaExceeded { .. }) => return Ok(Outcome::Refused),
        Err(other) => return Err(other.into()),
    }
    ledger.release_reservation(tx, account, size).await?;
    ledger.adjust_used(tx, account, size).await?;
    Ok(Outcome::Granted(size))
}

async fn apply(tx: &mut DbTx, account: Uuid, op: Op) -> Result<Outcome, Abort> {
    match op {
        Op::Commit { size } => commit_shaped(tx, account, size).await,
        Op::CommitRolledBack { size } => {
            let inner = tx
                .savepoint(async |tx| match commit_shaped(tx, account, size).await? {
                    Outcome::Granted(_) => Err(Abort::Rollback),
                    other => Ok(other),
                })
                .await;
            match inner {
                Err(Abort::Rollback) => Ok(Outcome::RolledBack),
                other => other,
            }
        }
        Op::Delete { size } => {
            ledger(GIB).adjust_used(tx, account, -size).await?;
            Ok(Outcome::Deleted)
        }
    }
}

fn describe(error: &RunError<Abort>) -> String {
    match error {
        RunError::Closure(Abort::Ledger(error)) => format!("ledger: {error:?}"),
        RunError::Closure(Abort::Db(error)) | RunError::Db(error) => format!("database: {error:?}"),
        RunError::Closure(Abort::Rollback) => "unexpected rollback marker".to_owned(),
    }
}

fn is_deadlock(error: &RunError<Abort>) -> bool {
    describe(error).contains("40P01")
}

async fn attempt(db: &Db, account: Uuid, op: Op) -> Result<Outcome, RunError<Abort>> {
    db.run(Scope::Account(account), async |tx| {
        apply(tx, account, op).await
    })
    .await
}

/// The outcome, or the error text after the one retry that 02 makes on a deadlock; and whether it retried.
async fn execute(db: &Db, account: Uuid, op: Op) -> (Result<Outcome, String>, bool) {
    match attempt(db, account, op).await {
        Err(error) if is_deadlock(&error) => (
            attempt(db, account, op)
                .await
                .map_err(|error| describe(&error)),
            true,
        ),
        other => (other.map_err(|error| describe(&error)), false),
    }
}

/// 20 waves of 50 transactions on `account`; `rollback_percent` of the commit-shaped ones end in a savepoint
/// rollback after their ledger calls.
pub async fn run(db: &Db, account: Uuid, rollback_percent: usize) -> Report {
    let mut rng = Rng(SEED);
    let mut live: Vec<i64> = Vec::new();
    let mut report = Report::default();
    for _ in 0..WAVES {
        let ops = plan_wave(&mut rng, &mut live, rollback_percent);
        let barrier = Arc::new(Barrier::new(ops.len()));
        let tasks: Vec<_> = ops
            .into_iter()
            .map(|op| {
                let (db, barrier) = (db.clone(), Arc::clone(&barrier));
                tokio::spawn(async move {
                    barrier.wait().await;
                    execute(&db, account, op).await
                })
            })
            .collect();
        for task in tasks {
            let (result, retried) = task.await.expect("workload task");
            report.retried += usize::from(retried);
            match result {
                Ok(Outcome::Granted(size)) => {
                    live.push(size);
                    report.granted += 1;
                }
                Ok(Outcome::Refused) => report.refused += 1,
                Ok(Outcome::RolledBack) => report.rolled_back += 1,
                Ok(Outcome::Deleted) => report.deleted += 1,
                Err(error) => report.errors.push(error),
            }
        }
    }
    report.ledger_sum = live.iter().sum();
    report
}

/// Reads the committed row every [`SAMPLE_EVERY`] on a connection of its own until stopped.
pub struct RowSampler {
    done: Arc<AtomicBool>,
    handle: JoinHandle<Vec<(i64, i64)>>,
}

impl RowSampler {
    pub fn start(mut conn: PgConnection, account: Uuid) -> RowSampler {
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&done);
        let handle = tokio::spawn(async move {
            let mut seen = Vec::new();
            loop {
                let (_, used, reserved) = quota_row(&mut conn, account).await;
                seen.push((used, reserved));
                if stop.load(Ordering::Acquire) {
                    break;
                }
                tokio::time::sleep(SAMPLE_EVERY).await;
            }
            conn.close().await.expect("close sampler connection");
            seen
        });
        RowSampler { done, handle }
    }

    /// Every `(used_bytes, reserved_bytes)` seen.
    pub async fn stop(self) -> Vec<(i64, i64)> {
        self.done.store(true, Ordering::Release);
        self.handle.await.expect("row sampler")
    }
}

const WAITS: &str = "SELECT (EXTRACT(EPOCH FROM clock_timestamp() - query_start) * 1000)::float8 \
     FROM pg_stat_activity \
     WHERE datname = current_database() AND application_name = $1 AND wait_event_type = 'Lock'";

/// Milliseconds the statements of sessions labelled `label` have been running while they wait for a lock,
/// read from `pg_stat_activity` on a superuser connection.
pub async fn lock_waits_ms(admin: &mut PgConnection, label: &str) -> Vec<f64> {
    sqlx::query_scalar(WAITS)
        .bind(label)
        .fetch_all(admin)
        .await
        .expect("read lock waits")
}

/// The longest lock wait of `label` sessions seen at any probe, and the number of probes.
pub struct WaitProbe {
    done: Arc<AtomicBool>,
    handle: JoinHandle<(f64, usize)>,
}

impl WaitProbe {
    pub fn start(mut admin: PgConnection, label: &'static str) -> WaitProbe {
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&done);
        let handle = tokio::spawn(async move {
            let (mut longest, mut probes) = (0.0_f64, 0);
            loop {
                for waited in lock_waits_ms(&mut admin, label).await {
                    longest = longest.max(waited);
                }
                probes += 1;
                if stop.load(Ordering::Acquire) {
                    break;
                }
                tokio::time::sleep(SAMPLE_EVERY).await;
            }
            admin.close().await.expect("close probe connection");
            (longest, probes)
        });
        WaitProbe { done, handle }
    }

    pub async fn stop(self) -> (f64, usize) {
        self.done.store(true, Ordering::Release);
        self.handle.await.expect("wait probe")
    }
}
