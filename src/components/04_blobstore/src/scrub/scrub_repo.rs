//! The statements of the scrubber. A scrub works across accounts, so every unit of work runs in system scope
//! (ADR-014). Each is one short transaction; none spans a file read, and the selection takes no row lock. The
//! paged walk of committed ids is shared with the missing-blob inventory.

use sc_db::{Db, DbError, RunError};
use uuid::Uuid;

/// The run's set, oldest first, on the partial scrub index. `$1` is the run start (a blob committed later belongs
/// to the next run), `$2` the age threshold, `$3` the blobs this run set aside, `$4` the batch.
const SELECT: &str = "SELECT id, size, wrapped_key, master_key_id FROM blobs \
     WHERE state = 'committed' \
         AND created_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
         AND (last_scrubbed_at IS NULL \
             OR last_scrubbed_at < 'epoch'::timestamptz + $2::bigint * interval '1 microsecond') \
         AND id <> ALL($3) \
     ORDER BY last_scrubbed_at NULLS FIRST, id \
     LIMIT $4";

/// The re-check and the mark are one statement under the row lock: a blob that a collector or a reinstate holds
/// is waited for, and the mark is written only to a row that is still `committed`. `first` is true for the
/// first finding of the blob, the only one that writes an audit event.
const MARK: &str = "WITH hit AS ( \
         SELECT id, unavailable_at IS NULL AS first FROM blobs \
         WHERE id = $1 AND state = 'committed' FOR UPDATE) \
     UPDATE blobs b \
     SET unavailable_at = COALESCE(b.unavailable_at, 'epoch'::timestamptz + $2::bigint * interval '1 microsecond') \
     FROM hit WHERE b.id = hit.id \
     RETURNING b.account_id, hit.first";

const SCRUBBED: &str = "UPDATE blobs \
     SET last_scrubbed_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
     WHERE id = ANY($1) AND state = 'committed'";

const STATUS: &str = "SELECT \
         (SELECT (EXTRACT(EPOCH FROM min(last_scrubbed_at)) * 1000000)::bigint \
          FROM blobs WHERE state = 'committed'), \
         (SELECT count(*) FROM blobs WHERE state = 'committed' AND last_scrubbed_at IS NULL), \
         (SELECT count(*) FROM blobs WHERE unavailable_at IS NOT NULL)";

/// One page of the inventory walk: committed ids in key order, after `$1` (all when null).
const COMMITTED_PAGE: &str = "SELECT id FROM blobs \
     WHERE state = 'committed' AND ($1::uuid IS NULL OR id > $1) \
     ORDER BY id LIMIT $2";

pub(super) struct Candidate {
    pub id: Uuid,
    pub size: i64,
    pub wrapped_key: Vec<u8>,
    pub master_key_id: i16,
}

pub(super) struct Marked {
    pub account: Uuid,
    pub first: bool,
}

pub(super) struct Counts {
    pub oldest_scrubbed_micros: Option<i64>,
    pub unscrubbed: i64,
    pub unavailable: i64,
}

pub(super) async fn select(
    db: &Db,
    started_micros: i64,
    threshold_micros: i64,
    set_aside: &[Uuid],
    limit: i64,
) -> Result<Vec<Candidate>, RunError<DbError>> {
    db.run_system("blob_scrub_select", async |tx| {
        let rows: Vec<(Uuid, i64, Vec<u8>, i16)> = sqlx::query_as(SELECT)
            .bind(started_micros)
            .bind(threshold_micros)
            .bind(set_aside)
            .bind(limit)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(
            rows.into_iter()
                .map(|(id, size, wrapped_key, master_key_id)| Candidate {
                    id,
                    size,
                    wrapped_key,
                    master_key_id,
                })
                .collect(),
        )
    })
    .await
}

/// `None` when the blob is no longer `committed` (collected, being collected, or gone).
pub(super) async fn recheck_and_mark(
    db: &Db,
    blob_id: Uuid,
    at_micros: i64,
) -> Result<Option<Marked>, RunError<DbError>> {
    db.run_system("blob_scrub_mark", async |tx| {
        let row: Option<(Uuid, bool)> = sqlx::query_as(MARK)
            .bind(blob_id)
            .bind(at_micros)
            .fetch_optional(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(row.map(|(account, first)| Marked { account, first }))
    })
    .await
}

pub(super) async fn mark_scrubbed(
    db: &Db,
    ids: &[Uuid],
    at_micros: i64,
) -> Result<(), RunError<DbError>> {
    db.run_system("blob_scrub_progress", async |tx| {
        sqlx::query(SCRUBBED)
            .bind(ids)
            .bind(at_micros)
            .execute(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(())
    })
    .await
}

pub(super) async fn counts(db: &Db) -> Result<Counts, RunError<DbError>> {
    db.run_system("blob_scrub_status", async |tx| {
        let (oldest, unscrubbed, unavailable): (Option<i64>, i64, i64) = sqlx::query_as(STATUS)
            .fetch_one(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(Counts {
            oldest_scrubbed_micros: oldest,
            unscrubbed,
            unavailable,
        })
    })
    .await
}

/// Up to `limit` committed ids greater than `after`, ascending.
pub(crate) async fn committed_page(
    db: &Db,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Uuid>, RunError<DbError>> {
    db.run_system("blob_inventory_page", async |tx| {
        let ids: Vec<Uuid> = sqlx::query_scalar(COMMITTED_PAGE)
            .bind(after)
            .bind(limit)
            .fetch_all(tx.conn())
            .await
            .map_err(DbError::from)?;
        Ok::<_, DbError>(ids)
    })
    .await
}
