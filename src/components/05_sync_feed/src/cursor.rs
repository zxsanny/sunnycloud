use contracts::{ChangeFeedError, Cursor, ResyncReason};

/// The account's feed row as the cursor rules see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedState {
    pub epoch: i64,
    pub seq: i64,
    pub purge_horizon: i64,
}

/// `E.S`: two non-negative decimal numbers that fit `i64`, split by one dot. Anything else, including a sign,
/// whitespace or a number too large, is `BadCursor`.
pub fn parse_cursor(text: &str) -> Result<Cursor, ChangeFeedError> {
    let (epoch, seq) = text.split_once('.').ok_or(ChangeFeedError::BadCursor)?;
    Ok(Cursor {
        epoch: parse_number(epoch)?,
        seq: parse_number(seq)?,
    })
}

fn parse_number(text: &str) -> Result<i64, ChangeFeedError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ChangeFeedError::BadCursor);
    }
    text.parse().map_err(|_| ChangeFeedError::BadCursor)
}

/// A cursor that breaks several rules gets the first reason in this order: epoch, horizon, future.
pub fn judge_cursor(cursor: Cursor, feed: FeedState) -> Result<(), ResyncReason> {
    if cursor.epoch != feed.epoch {
        Err(ResyncReason::EpochChanged)
    } else if cursor.seq < feed.purge_horizon {
        Err(ResyncReason::BelowPurgeHorizon)
    } else if cursor.seq > feed.seq {
        Err(ResyncReason::CursorAhead)
    } else {
        Ok(())
    }
}

/// The requested size, or the default, capped by the ceiling when one is set; at least one item.
pub fn page_limit(requested: Option<u32>, default: u64, ceiling: Option<u64>) -> i64 {
    let wanted = requested.map_or(default, u64::from);
    let capped = ceiling.map_or(wanted, |max| wanted.min(max));
    i64::try_from(capped.max(1)).unwrap_or(i64::MAX)
}
