//! 05 sync feed: the per-account change counter and the change feed. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section, the failure conversions of the feed ports and the stamp;
//! the notifier, the horizon and the reads are implemented by later tasks. The schema is
//! `src/migrations/*_create_feed_tables.*.sql`: `account_feeds`, and `nodes.seq` with `nodes_feed_idx` on the
//! tree's table.

mod config;
mod error;
mod stamp_repo;

pub use config::{FeedConfig, LONGPOLL_WAIT_MAX_CEILING, LONGPOLL_WAIT_MAX_FLOOR};
pub use error::{EmptyTouchedNodes, account_feed_missing, horizon_error, read_error, stamp_error};
pub use stamp_repo::{PgFeedStamp, touched_nodes};
