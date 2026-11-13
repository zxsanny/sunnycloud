//! Statements of the cascade and the tombstone purge (`TreeMaintenance`). Both run in system scope, since they
//! serve every account: the claim of a pending folder or of due leaves is the one statement that does not know
//! the account, and every statement after it names `account_id`. Claims use `FOR UPDATE SKIP LOCKED`, so two
//! runs never take the same rows and no leader is needed.

use sc_clock::Timestamp;
use sc_db::{Db, DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

use crate::retry::{TxFailure, run_failure};

/// A tombstoned node with no child row, due for the purge.
pub(crate) struct Leaf {
    pub id: Uuid,
    pub account: Uuid,
    pub seq: i64,
}

const CLAIM_PENDING: &str = "SELECT id, account_id FROM nodes WHERE cascade_pending \
    ORDER BY account_id, id LIMIT 1 FOR UPDATE SKIP LOCKED";

// Children are locked without SKIP LOCKED: a skipped live child would let the flag clear with the child still live.
// A subfolder gets `cascade_pending` itself, so the next level is marked by a later claim. No `seq` is written.
// The pick is a materialized CTE, evaluated once: as an `IN (... LIMIT)` subquery the planner may rescan it while the update
// runs and mark more than the limit.
const MARK_CHILDREN: &str = "WITH picked AS MATERIALIZED ( \
    SELECT id FROM nodes WHERE account_id = $1 AND parent_id = $2 AND deleted_at IS NULL \
    ORDER BY id LIMIT $4 FOR UPDATE) \
    UPDATE nodes n SET \
    deleted_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
    updated_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
    cascade_pending = (n.kind = 'folder') \
    FROM picked WHERE n.account_id = $1 AND n.id = picked.id";

const CLEAR_PENDING: &str = "UPDATE nodes SET cascade_pending = false \
    WHERE account_id = $1 AND id = $2 AND NOT EXISTS ( \
    SELECT 1 FROM nodes c WHERE c.account_id = $1 AND c.parent_id = $2 AND c.deleted_at IS NULL)";

// Leaf first: a node with any child row, tombstoned or not, waits until its children are gone.
const CLAIM_LEAVES: &str = "SELECT n.id, n.account_id, n.seq FROM nodes n \
    WHERE n.deleted_at IS NOT NULL \
    AND n.deleted_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
    AND NOT EXISTS (SELECT 1 FROM nodes c WHERE c.account_id = n.account_id AND c.parent_id = n.id) \
    ORDER BY n.deleted_at, n.id LIMIT $2 FOR UPDATE OF n SKIP LOCKED";

const REVISIONS_OF: &str =
    "SELECT id, blob_id FROM revisions WHERE account_id = $1 AND node_id = ANY($2) ORDER BY id";

const DELETE_NODES: &str = "DELETE FROM nodes WHERE account_id = $1 AND id = ANY($2)";

/// Runs `f` in one system-scope transaction named `op` in `pg_stat_activity`.
pub(crate) async fn in_system<T, E: TxFailure>(
    db: &Db,
    op: &'static str,
    f: impl AsyncFnOnce(&mut DbTx) -> Result<T, E>,
) -> Result<T, E> {
    db.run_system(op, f).await.map_err(run_failure)
}

/// One folder with `cascade_pending` that no other run holds, locked: `(folder, account)`.
pub(crate) async fn claim_pending(tx: &mut DbTx) -> Result<Option<(Uuid, Uuid)>, DbError> {
    let row = sqlx::query(CLAIM_PENDING).fetch_optional(tx.conn()).await?;
    row.map(|row| Ok((row.try_get(0)?, row.try_get(1)?)))
        .transpose()
}

/// Tombstones up to `limit` live children of `folder` at `now`; returns how many.
pub(crate) async fn mark_children(
    tx: &mut DbTx,
    account: Uuid,
    folder: Uuid,
    now: Timestamp,
    limit: u64,
) -> Result<u64, DbError> {
    let done = sqlx::query(MARK_CHILDREN)
        .bind(account)
        .bind(folder)
        .bind(now.as_unix_micros())
        .bind(bound(limit))
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// Clears the folder's flag when no live child is left; `true` when it did.
pub(crate) async fn clear_pending(
    tx: &mut DbTx,
    account: Uuid,
    folder: Uuid,
) -> Result<bool, DbError> {
    let done = sqlx::query(CLEAR_PENDING)
        .bind(account)
        .bind(folder)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Up to `limit` leaves tombstoned at or before `cutoff` that no other run holds, locked.
pub(crate) async fn claim_leaves(
    tx: &mut DbTx,
    cutoff: Timestamp,
    limit: u64,
) -> Result<Vec<Leaf>, DbError> {
    let rows = sqlx::query(CLAIM_LEAVES)
        .bind(cutoff.as_unix_micros())
        .bind(bound(limit))
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(Leaf {
                id: row.try_get(0)?,
                account: row.try_get(1)?,
                seq: row.try_get(2)?,
            })
        })
        .collect()
}

/// Every revision of the nodes, current or not, with its blob; no row lock: the caller locks each blob row first.
pub(crate) async fn revisions_of(
    tx: &mut DbTx,
    account: Uuid,
    nodes: &[Uuid],
) -> Result<Vec<crate::lifecycle_repo::Owned>, DbError> {
    let rows = sqlx::query(REVISIONS_OF)
        .bind(account)
        .bind(nodes)
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(crate::lifecycle_repo::Owned {
                revision_id: row.try_get(0)?,
                blob_id: row.try_get(1)?,
            })
        })
        .collect()
}

pub(crate) async fn delete_nodes(
    tx: &mut DbTx,
    account: Uuid,
    ids: &[Uuid],
) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_NODES)
        .bind(account)
        .bind(ids)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// A `LIMIT` value; a batch limit comes from a `u32`, so the cap is never reached.
fn bound(limit: u64) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}
