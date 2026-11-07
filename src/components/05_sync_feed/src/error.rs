use contracts::{ChangeFeedError, FeedHorizonError, FeedStampError};
use sc_db::DbError;
use sc_observability::log_error;
use uuid::Uuid;

/// The account has no feed row: an invariant violation, since the account lifecycle creates the row with the
/// account. Logged at ERROR with the account id.
pub fn account_feed_missing(account: Uuid) -> FeedStampError {
    log_error!("account feed row missing", account_id = account);
    FeedStampError::AccountFeedMissing
}

/// Every database failure of the stamp is the unexpected tier: its text goes to the log through the source,
/// never into the message.
pub fn stamp_error(error: DbError) -> FeedStampError {
    FeedStampError::Unexpected(Box::new(error))
}

pub fn horizon_error(error: DbError) -> FeedHorizonError {
    FeedHorizonError::Unexpected(Box::new(error))
}

pub fn read_error(error: DbError) -> ChangeFeedError {
    ChangeFeedError::Unexpected(Box::new(error))
}

/// A touched set with no node: with nothing changed there is no stamp.
#[derive(Debug, thiserror::Error)]
#[error("touched nodes are empty")]
pub struct EmptyTouchedNodes;

/// A read or head of an account without a feed row: an invariant violation, like the stamp's.
#[derive(Debug, thiserror::Error)]
#[error("account feed missing")]
pub struct FeedRowMissing;

/// A node row the contract types cannot hold (an unknown kind, a hash that is not 32 bytes).
#[derive(Debug, thiserror::Error)]
#[error("node row holds an unexpected {0}")]
pub struct BadNodeRow(pub &'static str);

pub fn feed_row_missing(account: Uuid) -> ChangeFeedError {
    log_error!("account feed row missing", account_id = account);
    ChangeFeedError::Unexpected(Box::new(FeedRowMissing))
}

pub fn bad_node_row(account: Uuid, what: &'static str) -> ChangeFeedError {
    log_error!("node row not representable", account_id = account);
    ChangeFeedError::Unexpected(Box::new(BadNodeRow(what)))
}
