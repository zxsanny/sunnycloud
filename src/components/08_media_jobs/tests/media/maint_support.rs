//! Seeding and probes of the maintenance tests (AZ-3845): job and derived rows written as the schema owner, with
//! committed blob rows under each derived row, and the GC queue rows the real 04 `GcQueue` wrote.

use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::queue_support::Queue;

/// `count` jobs of `account` over its `revision`, cycling through queued, running, succeeded and dead.
pub async fn seed_jobs(conn: &mut PgConnection, account: Uuid, revision: Uuid, count: i32) {
    sqlx::query(
        "INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
         attempts, available_at, created_at, finished_at, outcome, leased_by) \
         SELECT gen_random_uuid(), $1, $2, 'stills', sha256(convert_to('job' || n::text, 'UTF8')), 1, 0, \
         (ARRAY['queued', 'running', 'succeeded', 'dead'])[n % 4 + 1], 1, 'epoch'::timestamptz, \
         'epoch'::timestamptz, CASE WHEN n % 4 >= 2 THEN 'epoch'::timestamptz END, \
         CASE WHEN n % 4 = 2 THEN 'derived' END, CASE WHEN n % 4 = 1 THEN 'w' END \
         FROM generate_series(1, $3) AS n",
    )
    .bind(account)
    .bind(revision)
    .bind(count)
    .execute(conn)
    .await
    .expect("seed jobs");
}

/// `count` derived rows of `account`, each over a committed blob row of its own; returns the blob ids. With
/// `content_hash` every row is of that content (one params version each), otherwise each has a content of its own
/// that no revision has.
pub async fn seed_derived(
    conn: &mut PgConnection,
    account: Uuid,
    content_hash: Option<&[u8]>,
    count: i32,
) -> Vec<Uuid> {
    let mut blobs: Vec<Uuid> = sqlx::query_scalar(
        "WITH b AS (INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         SELECT gen_random_uuid(), $1, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz \
         FROM generate_series(1, $3) RETURNING id), \
         numbered AS (SELECT id, row_number() OVER (ORDER BY id) AS n FROM b) \
         INSERT INTO derived (account_id, content_hash, kind, params_version, blob_id, media_type, size, created_at) \
         SELECT $1, coalesce($2, sha256(convert_to(id::text, 'UTF8'))), 'thumb', n::smallint, id, 'image/webp', 10, \
         'epoch'::timestamptz FROM numbered RETURNING blob_id",
    )
    .bind(account)
    .bind(content_hash)
    .bind(count)
    .fetch_all(conn)
    .await
    .expect("seed derived");
    blobs.sort();
    blobs
}

/// The account's GC queue rows: blob id, reason, and whether the row is due at `now_micros`.
pub async fn gc_rows(q: &Queue, account: Uuid) -> Vec<(Uuid, String, bool)> {
    let mut conn = q.fixture.owner_conn().await;
    let rows = sqlx::query_as(
        "SELECT g.blob_id, g.reason, g.due_at = ('epoch'::timestamptz + $2::bigint * interval '1 microsecond') \
         FROM blob_gc g JOIN blobs b ON b.id = g.blob_id AND b.state = 'gc_pending' \
         WHERE g.account_id = $1 ORDER BY g.blob_id",
    )
    .bind(account)
    .bind(q.clock_now().as_unix_micros())
    .fetch_all(&mut conn)
    .await
    .expect("gc rows");
    conn.close().await.expect("close owner connection");
    rows
}

/// (jobs, derived rows) of `account`.
pub async fn rows_of(q: &Queue, account: Uuid) -> (i64, i64) {
    let mut conn = q.fixture.owner_conn().await;
    let counts = sqlx::query_as(
        "SELECT (SELECT count(*) FROM jobs WHERE account_id = $1), \
         (SELECT count(*) FROM derived WHERE account_id = $1)",
    )
    .bind(account)
    .fetch_one(&mut conn)
    .await
    .expect("row counts");
    conn.close().await.expect("close owner connection");
    counts
}

/// The blob ids of the derived rows left, over every account.
pub async fn derived_blobs(q: &Queue) -> Vec<Uuid> {
    let mut conn = q.fixture.owner_conn().await;
    let blobs = sqlx::query_scalar("SELECT blob_id FROM derived ORDER BY blob_id")
        .fetch_all(&mut conn)
        .await
        .expect("derived blobs");
    conn.close().await.expect("close owner connection");
    blobs
}

/// A `worker_status` row seen at `seen_micros`.
pub async fn worker_seen(q: &Queue, worker: &str, seen_micros: i64) {
    let mut conn = q.fixture.owner_conn().await;
    sqlx::query(
        "INSERT INTO worker_status (worker_id, seen_at, image_age_days, decoder_ok) \
         VALUES ($1, 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 3, true)",
    )
    .bind(worker)
    .bind(seen_micros)
    .execute(&mut conn)
    .await
    .expect("insert worker status");
    conn.close().await.expect("close owner connection");
}
