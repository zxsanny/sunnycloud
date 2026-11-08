use contracts::{FeedHorizon, FeedHorizonError, Seq, async_trait};
use sc_db::DbTx;
use uuid::Uuid;

use crate::error::horizon_error;

/// Non-decreasing: a lower `seq` keeps the horizon (a lowered horizon would accept cursors whose tombstones
/// were purged). No row, as for an account being erased, updates nothing.
const RAISE: &str = "\
UPDATE account_feeds SET purge_horizon = GREATEST(purge_horizon, $2) WHERE account_id = $1";

/// Raises the purge horizon in the caller's transaction. Stateless; the transaction carries the account scope.
#[derive(Debug, Clone, Copy, Default)]
pub struct PgFeedHorizon;

#[async_trait]
impl FeedHorizon for PgFeedHorizon {
    async fn raise_purge_horizon(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        seq: Seq,
    ) -> Result<(), FeedHorizonError> {
        sqlx::query(RAISE)
            .bind(account)
            .bind(seq)
            .execute(tx.conn())
            .await
            .map_err(|error| horizon_error(error.into()))?;
        Ok(())
    }
}
