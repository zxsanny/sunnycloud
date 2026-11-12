//! The media effect over the job queue of a per-test database (AZ-3841). Each entry is backed by a real revision
//! row of the account's file, because a job names its revision; the effect runs in the account's scope, as inside
//! 02's commit transaction. Rows are read as the schema owner.

use contracts::{CommitEffect, EffectError, PublishedEntry};
use media_api::{JobPlanner, MediaEnqueueEffect};
use media_shared::policy::CURRENT_PARAMS_VERSION;
use sc_db::{RunError, Scope};
use sc_ids::FileHash;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::queue_support::{Queue, config, hash};
use crate::support::{DerivedRow, blob, derived};

pub const THRESHOLD: u64 = 5;
pub const JPEG: Option<&str> = Some("image/jpeg");
pub const MP4: Option<&str> = Some("video/mp4");

/// One job row as the effect left it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Job {
    pub kind: String,
    pub lane: String,
    pub priority: i32,
    pub params_version: i32,
}

impl Job {
    pub fn new(kind: &str, lane: &str, priority: i32) -> Job {
        Job {
            kind: kind.to_owned(),
            lane: lane.to_owned(),
            priority,
            params_version: 1,
        }
    }
}

/// The effect of `account` at `params_version` with threshold 5. It holds the queue's pool: drop it before
/// `Queue::finish`.
pub fn effect(q: &Queue, account: Uuid, params_version: u32) -> MediaEnqueueEffect {
    MediaEnqueueEffect::new(
        q.queue.clone(),
        JobPlanner::new(params_version, THRESHOLD),
        account,
    )
}

/// The effect of the default account with the current params version and threshold 5.
pub fn current(q: &Queue) -> MediaEnqueueEffect {
    effect(q, q.account, CURRENT_PARAMS_VERSION)
}

/// The effect of the default account with the configured threshold (200), so no entry turns bulk by count.
pub fn configured(q: &Queue) -> MediaEnqueueEffect {
    MediaEnqueueEffect::new(
        q.queue.clone(),
        JobPlanner::from_config(&config()),
        q.account,
    )
}

/// What the published entry of a revision carries; `vault` and `import_hint` are set by the caller.
pub struct Content {
    pub hash: FileHash,
    pub media_type: Option<&'static str>,
    pub size: i64,
}

impl Content {
    pub fn new(n: u32, media_type: Option<&'static str>) -> Content {
        Content {
            hash: hash(n),
            media_type,
            size: 100,
        }
    }
}

async fn owner(q: &Queue) -> PgConnection {
    q.fixture.owner_conn().await
}

/// A new revision of the account's file over `content` (a fresh blob), and its published entry.
pub async fn entry_of(q: &Queue, account: Uuid, content: &Content) -> PublishedEntry {
    let mut conn = owner(q).await;
    let node: Uuid =
        sqlx::query_scalar("SELECT node_id FROM revisions WHERE account_id = $1 LIMIT 1")
            .bind(account)
            .fetch_one(&mut conn)
            .await
            .expect("the account's file");
    let (blob_id, revision_id) = (Uuid::new_v4(), Uuid::new_v4());
    blob(&mut conn, blob_id, account).await;
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, media_type, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, '\\x', $7, 'epoch'::timestamptz)",
    )
    .bind(revision_id)
    .bind(account)
    .bind(node)
    .bind(blob_id)
    .bind(content.size)
    .bind(content.hash.as_bytes().as_slice())
    .bind(content.media_type)
    .execute(&mut conn)
    .await
    .expect("insert revision");
    conn.close().await.expect("close owner connection");
    PublishedEntry {
        node_id: node,
        revision_id,
        content_hash: content.hash,
        media_type: content.media_type.map(str::to_owned),
        size: content.size,
        import_hint: false,
        vault: false,
    }
}

pub async fn entry(q: &Queue, content: &Content) -> PublishedEntry {
    entry_of(q, q.account, content).await
}

/// `on_entry` in the entry account's own transaction, committed.
pub async fn run(
    q: &Queue,
    effect: &MediaEnqueueEffect,
    account: Uuid,
    entry: &PublishedEntry,
) -> Result<(), RunError<EffectError>> {
    q.fixture
        .api
        .run(Scope::Account(account), async |tx| {
            effect.on_entry(tx, entry).await
        })
        .await
}

/// The jobs of `account` for `content_hash`, by kind and params version.
pub async fn jobs_of(q: &Queue, account: Uuid, content_hash: &FileHash) -> Vec<Job> {
    let mut conn = owner(q).await;
    let jobs = sqlx::query_as(
        "SELECT kind, lane, priority::int, params_version::int FROM jobs \
         WHERE account_id = $1 AND content_hash = $2 ORDER BY kind, params_version",
    )
    .bind(account)
    .bind(content_hash.as_bytes().as_slice())
    .fetch_all(&mut conn)
    .await
    .expect("job rows");
    conn.close().await.expect("close owner connection");
    jobs
}

pub async fn jobs(q: &Queue, content_hash: &FileHash) -> Vec<Job> {
    jobs_of(q, q.account, content_hash).await
}

/// A derived row of the default account for `content_hash`, kind `kind`, params version 1, over a fresh blob.
pub async fn add_derived(q: &Queue, content_hash: &FileHash, kind: &str) {
    let mut conn = owner(q).await;
    let blob_id = Uuid::new_v4();
    blob(&mut conn, blob_id, q.account).await;
    derived(
        &mut conn,
        &DerivedRow {
            account: q.account,
            content_hash: content_hash.as_bytes(),
            kind,
            params_version: 1,
            blob: blob_id,
        },
    )
    .await
    .expect("insert derived row");
    conn.close().await.expect("close owner connection");
}
