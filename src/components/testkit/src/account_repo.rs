//! The two statements of the `AccountGuard` double, on the transaction's connection.

use sc_db::DbTx;
use uuid::Uuid;

const NOTHING_WRITTEN_YET: &str = "SELECT pg_current_xact_id_if_assigned() IS NULL";

const STATE_FOR_SHARE: &str = "SELECT state FROM accounts WHERE id = $1 FOR SHARE";

/// `true` when the transaction has not written or locked a row yet.
pub(crate) async fn nothing_written_yet(tx: &mut DbTx) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(NOTHING_WRITTEN_YET)
        .fetch_one(tx.conn())
        .await
}

/// The account's state, locked `FOR SHARE`; `None` for an unknown account.
pub(crate) async fn state_for_share(
    tx: &mut DbTx,
    account: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(STATE_FOR_SHARE)
        .bind(account)
        .fetch_optional(tx.conn())
        .await
}
