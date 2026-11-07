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
