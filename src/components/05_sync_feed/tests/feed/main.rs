//! Sync-feed skeleton (AZ-3774): settings, failure conversions and the `account_feeds` and `nodes.seq` migration
//! on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod config;
mod errors;
mod migration;
mod rls;
mod support;
mod system_scope_repo;
