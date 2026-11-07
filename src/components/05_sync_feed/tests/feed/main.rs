//! Sync feed: settings, failure conversions, the `account_feeds` and `nodes.seq` migration (AZ-3774) and the stamp
//! (AZ-3775) and the change feed read (AZ-3776) on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod config;
mod cursor;
mod errors;
mod migration;
mod read;
mod read_concurrency;
mod read_plan;
mod read_support;
mod rls;
mod stamp;
mod stamp_concurrency;
mod statements;
mod support;
mod system_scope_repo;
