//! Statements of the account lifecycle hook and of the blob owner cleanup. The root insert and the erase
//! statements name `account_id`; the owner cleanup runs in the GC sweeper's system transaction and finds the
//! revision by `blob_id` alone, which is unique across accounts (`revisions_blob_id_key`).

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

/// A revision the erase removes, with the blob it queues.
pub(crate) struct Owned {
    pub revision_id: Uuid,
    pub blob_id: Uuid,
}

// The unique index `nodes_root_key` refuses a second root of the account.
const INSERT_ROOT: &str = "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, \
    name_key_version, created_at, updated_at) VALUES ($1, $2, NULL, 'folder', '', '', $3, \
    'epoch'::timestamptz + $4::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $4::bigint * interval '1 microsecond')";

const REVISION_BATCH: &str =
    "SELECT id, blob_id FROM revisions WHERE account_id = $1 ORDER BY id LIMIT $2";

const DELETE_REVISIONS: &str = "DELETE FROM revisions WHERE account_id = $1 AND id = ANY($2)";

// A leaf has no child row. The root is a leaf only once every other node of the account is gone, so it goes last.
const DELETE_LEAVES: &str = "DELETE FROM nodes WHERE account_id = $1 AND id IN ( \
    SELECT n.id FROM nodes n WHERE n.account_id = $1 \
    AND NOT EXISTS (SELECT 1 FROM nodes c WHERE c.parent_id = n.id) \
    ORDER BY n.id LIMIT $2)";

const TREE_ROWS: &str = "SELECT (SELECT count(*) FROM nodes WHERE account_id = $1), \
    (SELECT count(*) FROM revisions WHERE account_id = $1)";

// `nodes.current_revision_id` is `ON DELETE SET NULL`, so a current revision goes too and its node keeps no pointer.
const DELETE_OWNER: &str = "DELETE FROM revisions WHERE blob_id = $1";

pub(crate) async fn insert_root(
    tx: &mut DbTx,
    id: Uuid,
    account: Uuid,
    now: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(INSERT_ROOT)
        .bind(id)
        .bind(account)
        .bind(sc_names::NAME_KEY_VERSION)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// Up to `limit` of the account's revisions, without a row lock: the caller locks each blob row first.
pub(crate) async fn revision_batch(
    tx: &mut DbTx,
    account: Uuid,
    limit: u64,
) -> Result<Vec<Owned>, DbError> {
    let rows = sqlx::query(REVISION_BATCH)
        .bind(account)
        .bind(bound(limit))
        .fetch_all(tx.conn())
        .await?;
    rows.iter()
        .map(|row| {
            Ok(Owned {
                revision_id: row.try_get(0)?,
                blob_id: row.try_get(1)?,
            })
        })
        .collect()
}

pub(crate) async fn delete_revisions(
    tx: &mut DbTx,
    account: Uuid,
    ids: &[Uuid],
) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_REVISIONS)
        .bind(account)
        .bind(ids)
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// Deletes up to `limit` nodes that have no child; the caller repeats it for the next level up.
pub(crate) async fn delete_leaves(
    tx: &mut DbTx,
    account: Uuid,
    limit: u64,
) -> Result<u64, DbError> {
    let done = sqlx::query(DELETE_LEAVES)
        .bind(account)
        .bind(bound(limit))
        .execute(tx.conn())
        .await?;
    Ok(done.rows_affected())
}

/// The account's `(nodes, revisions)` row counts.
pub(crate) async fn tree_rows(tx: &mut DbTx, account: Uuid) -> Result<(i64, i64), DbError> {
    let row = sqlx::query(TREE_ROWS)
        .bind(account)
        .fetch_one(tx.conn())
        .await?;
    Ok((row.try_get(0)?, row.try_get(1)?))
}

pub(crate) async fn delete_owner(tx: &mut DbTx, blob_id: Uuid) -> Result<(), DbError> {
    sqlx::query(DELETE_OWNER)
        .bind(blob_id)
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// A `LIMIT` value; a batch limit comes from a `u32`, so the cap is never reached.
fn bound(limit: u64) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}
