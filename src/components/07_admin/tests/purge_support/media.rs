//! The media part of a purger account (08's tables), written as the schema owner: photo and video jobs over the
//! account's revisions in every status, and derived rows (thumbnails, previews, transcodes) each over a committed
//! blob row of the account. The caller puts the derived blobs' files on the volume.

use sqlx::PgConnection;
use uuid::Uuid;

/// `count` jobs of `account`, cycling through the kinds `stills`, `poster` and `transcode` and the statuses
/// queued, running, succeeded and dead, spread over the account's revisions.
pub async fn jobs(conn: &mut PgConnection, account: Uuid, count: usize) {
    sqlx::query(
        "WITH revs AS (SELECT array_agg(id ORDER BY id) AS ids FROM revisions WHERE account_id = $1) \
         INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, \
         attempts, available_at, created_at, finished_at, outcome, leased_by) \
         SELECT gen_random_uuid(), $1, revs.ids[(n - 1) % cardinality(revs.ids) + 1], \
         (ARRAY['stills', 'poster', 'transcode'])[n % 3 + 1], sha256(convert_to('job' || n::text, 'UTF8')), 1, 0, \
         (ARRAY['queued', 'running', 'succeeded', 'dead'])[n % 4 + 1], 1, 'epoch'::timestamptz, \
         'epoch'::timestamptz, CASE WHEN n % 4 >= 2 THEN 'epoch'::timestamptz END, \
         CASE WHEN n % 4 = 2 THEN 'derived' END, CASE WHEN n % 4 = 1 THEN 'w' END \
         FROM revs, generate_series(1, $2) AS n",
    )
    .bind(account)
    .bind(i32::try_from(count).expect("fits"))
    .execute(conn)
    .await
    .expect("insert jobs");
}

/// `count` derived rows of `account`, each over a committed blob row of its own; returns the blob ids.
pub async fn derived(conn: &mut PgConnection, account: Uuid, count: usize) -> Vec<Uuid> {
    sqlx::query_scalar(
        "WITH b AS (INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         SELECT gen_random_uuid(), $1, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz \
         FROM generate_series(1, $2) RETURNING id), \
         numbered AS (SELECT id, row_number() OVER (ORDER BY id) AS n FROM b) \
         INSERT INTO derived (account_id, content_hash, kind, params_version, blob_id, media_type, size, created_at) \
         SELECT $1, sha256(convert_to(id::text, 'UTF8')), (ARRAY['thumb', 'preview', 'transcode'])[n % 3 + 1], \
         1, id, CASE WHEN n % 3 = 2 THEN 'video/mp4' ELSE 'image/webp' END, 1, 'epoch'::timestamptz \
         FROM numbered RETURNING blob_id",
    )
    .bind(account)
    .bind(i32::try_from(count).expect("fits"))
    .fetch_all(conn)
    .await
    .expect("insert derived rows")
}
