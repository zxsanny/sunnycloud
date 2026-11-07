use contracts::{FeedStamp, FeedStampError, Seq, TouchedNodes, async_trait};
use sc_db::DbTx;
use uuid::Uuid;

use crate::error::{EmptyTouchedNodes, account_feed_missing, stamp_error};

/// One data-modifying statement. The counter update takes the row lock of the account's feed row, which is the
/// serialisation point of its commits; the node update is a sibling CTE, so it runs even though the final
/// SELECT reads only the counter. No feed row means no counter row and so no node is touched.
const STAMP: &str = "\
WITH bumped AS (
    UPDATE account_feeds SET seq = seq + 1 WHERE account_id = $1 RETURNING seq
), stamped AS (
    UPDATE nodes SET seq = bumped.seq FROM bumped
    WHERE nodes.account_id = $1 AND nodes.id = ANY($2)
    RETURNING nodes.id
)
SELECT seq FROM bumped";

/// The only writer of `nodes.seq`. Stateless; the caller's transaction carries the account scope.
#[derive(Debug, Clone, Copy, Default)]
pub struct PgFeedStamp;

/// Builds the touched set, refusing an empty list.
///
/// `contracts::TouchedNodes` has a public field, so this check is by convention until that type gets a
/// checked constructor; `stamp` repeats the check.
pub fn touched_nodes(nodes: Vec<Uuid>) -> Result<TouchedNodes, EmptyTouchedNodes> {
    if nodes.is_empty() {
        return Err(EmptyTouchedNodes);
    }
    Ok(TouchedNodes(nodes))
}

#[async_trait]
impl FeedStamp for PgFeedStamp {
    async fn stamp(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        touched: TouchedNodes,
    ) -> Result<Seq, FeedStampError> {
        let touched = touched_nodes(touched.0)
            .map_err(|empty| FeedStampError::Unexpected(Box::new(empty)))?;
        sqlx::query_scalar(STAMP)
            .bind(account)
            .bind(touched.0)
            .fetch_optional(tx.conn())
            .await
            .map_err(|error| stamp_error(error.into()))?
            .ok_or_else(|| account_feed_missing(account))
    }
}
