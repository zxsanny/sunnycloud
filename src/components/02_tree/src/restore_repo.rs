//! Statements of the operator restore (`TreeRestore`): the revision of a node current at an instant, and the
//! pointer a tombstoned node loses when its revision moves to `Restored`. The restore runs in system scope, so
//! every statement names `account_id`: a foreign id behaves like a random one.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sqlx::Row;
use uuid::Uuid;

/// The revision a restore brings back.
pub(crate) struct Picked {
    pub id: Uuid,
    pub blob_id: Uuid,
    pub size: i64,
}

// Served by `revisions_node_id_created_at_idx`; two revisions of one instant are ordered by id, newest first.
const REVISION_AT: &str = "SELECT id, blob_id, size FROM revisions \
    WHERE account_id = $1 AND node_id = $2 \
    AND created_at <= 'epoch'::timestamptz + $3::bigint * interval '1 microsecond' \
    ORDER BY created_at DESC, id DESC LIMIT 1";

const CLEAR_CURRENT: &str = "UPDATE nodes SET current_revision_id = NULL \
    WHERE id = $1 AND account_id = $2 AND current_revision_id = $3";

/// The latest revision of `node` created at or before `at`.
pub(crate) async fn revision_at(
    tx: &mut DbTx,
    account: Uuid,
    node: Uuid,
    at: Timestamp,
) -> Result<Option<Picked>, DbError> {
    let row = sqlx::query(REVISION_AT)
        .bind(account)
        .bind(node)
        .bind(at.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?;
    row.map(|row| {
        Ok(Picked {
            id: row.try_get(0)?,
            blob_id: row.try_get(1)?,
            size: row.try_get(2)?,
        })
    })
    .transpose()
}

/// Clears `node`'s current revision when it is `revision`, which now belongs to another node.
pub(crate) async fn clear_current(
    tx: &mut DbTx,
    account: Uuid,
    node: Uuid,
    revision: Uuid,
) -> Result<(), DbError> {
    sqlx::query(CLEAR_CURRENT)
        .bind(node)
        .bind(account)
        .bind(revision)
        .execute(tx.conn())
        .await?;
    Ok(())
}
