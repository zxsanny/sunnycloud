//! 05 sync feed: the per-account change counter and the change feed. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section, the failure conversions of the feed ports, the stamp and
//! the immediate read path (cursor rules, page, head); the notifier, the horizon and the held read are
//! implemented by later tasks, so `ChangeFeed` itself is not implemented yet. The schema is
//! `src/migrations/*_create_feed_tables.*.sql`: `account_feeds`, and `nodes.seq` with `nodes_feed_idx` on the
//! tree's table.

mod config;
mod cursor;
mod error;
mod read_repo;
mod stamp_repo;

pub use config::{FeedConfig, LONGPOLL_WAIT_MAX_CEILING, LONGPOLL_WAIT_MAX_FLOOR};
pub use cursor::{FeedState, judge_cursor, page_limit, parse_cursor};
pub use error::{
    BadNodeRow, EmptyTouchedNodes, FeedRowMissing, account_feed_missing, horizon_error, read_error,
    stamp_error,
};
pub use read_repo::{PgFeedReader, READ_PAGE_SQL};
pub use stamp_repo::{PgFeedStamp, touched_nodes};
