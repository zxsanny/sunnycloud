//! The other accounts of PT-07-2: each commits one small file per second through 03's real upload and commit
//! service (`stage_small`, then a batch of one under the account's root), the path a client's small-file upload
//! takes behind the API. Every commit is timed from the stage to the commit result, with the instant it started,
//! so the caller splits the times into the baseline (before the deletion request) and the purge window.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use contracts::{BatchCommit, BatchEntry, BatchEntryResult, ByteStream, CommitActor};
use sc_clock::{CancellationToken, Clock};
use sc_ids::{hash_block, new_id};
use sqlx::PgConnection;
use sunnycloud_transfer::PgUploadService;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

use crate::purge_support::seed;
use crate::purge_support::world::PurgeWorld;

const COMMIT_EVERY: Duration = Duration::from_secs(1);
const DEVICE_LABEL: &str = "pt-07-2";

/// One other account and the folder its commits go to.
#[derive(Debug, Clone, Copy)]
pub struct Other {
    pub account: Uuid,
    root: Uuid,
}

#[derive(Default)]
pub struct Commits {
    /// `(started, took)` of each committed file.
    pub done: Vec<(Instant, Duration)>,
    pub errors: Vec<String>,
}

/// `count` active accounts, each seeded with one file (the purger suite's seed); none is the deletion target.
pub async fn others(world: &PurgeWorld, count: usize) -> Vec<Other> {
    let mut conn = world.fixture().await;
    let mut others = Vec::with_capacity(count);
    for _ in 0..count {
        let account = seed::account(world, seed::files(1)).await.account;
        others.push(Other {
            account,
            root: root_of(&mut conn, account).await,
        });
    }
    others
}

async fn root_of(conn: &mut PgConnection, account: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM nodes WHERE account_id = $1 AND parent_id IS NULL")
        .bind(account)
        .fetch_one(conn)
        .await
        .expect("read the root folder")
}

/// Live files of `account`.
pub async fn live_files(conn: &mut PgConnection, account: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE account_id = $1 AND kind = 'file' AND deleted_at IS NULL",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count live files")
}

/// Starts one committer per account; each commits a file every second until `stop` is cancelled.
pub fn start(
    upload: Arc<PgUploadService>,
    clock: Arc<dyn Clock>,
    others: &[Other],
    stop: &CancellationToken,
) -> Vec<JoinHandle<Commits>> {
    others
        .iter()
        .map(|&other| {
            let (upload, clock, stop) = (upload.clone(), clock.clone(), stop.clone());
            tokio::spawn(committer(upload, clock, other, stop))
        })
        .collect()
}

async fn committer(
    upload: Arc<PgUploadService>,
    clock: Arc<dyn Clock>,
    other: Other,
    stop: CancellationToken,
) -> Commits {
    let mut ticker = tokio::time::interval(COMMIT_EVERY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut commits = Commits::default();
    for n in 0.. {
        tokio::select! {
            biased;
            () = stop.cancelled() => break,
            _ = ticker.tick() => {}
        }
        let started = Instant::now();
        match commit_one(&upload, clock.as_ref(), other, n).await {
            Ok(()) => commits.done.push((started, started.elapsed())),
            Err(error) => commits.errors.push(error),
        }
    }
    commits
}

async fn commit_one(
    upload: &PgUploadService,
    clock: &dyn Clock,
    other: Other,
    n: u64,
) -> Result<(), String> {
    let data = format!("pt-07-2 {} {n}", other.account).into_bytes();
    let (staged_key, length, hash) = (new_id(clock), data.len() as u64, hash_block(&data));
    let body: ByteStream = Box::pin(futures_util::stream::iter([Ok(Bytes::from(data))]));
    upload
        .stage_small(other.account, staged_key, Some(hash), body, length)
        .await
        .map_err(|error| format!("stage {n}: {error:?}"))?;
    let result = upload
        .commit_batch(
            CommitActor {
                account_id: other.account,
                device_label: DEVICE_LABEL.to_owned(),
            },
            BatchCommit {
                commit_id: new_id(clock),
                entries: vec![BatchEntry {
                    staged_key,
                    parent_id: other.root,
                    name: format!("small-{n}.txt"),
                    base_revision: None,
                    client_mtime: None,
                    client_meta: None,
                    import_hint: false,
                }],
            },
        )
        .await
        .map_err(|error| format!("commit {n}: {error:?}"))?;
    match result.per_entry.as_slice() {
        [BatchEntryResult::Committed(_)] => Ok(()),
        other => Err(format!("commit {n}: {other:?}")),
    }
}
