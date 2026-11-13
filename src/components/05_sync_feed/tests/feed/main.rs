//! Sync feed: settings, failure conversions, the `account_feeds` and `nodes.seq` migration (AZ-3774) and the stamp
//! (AZ-3775), the change feed read (AZ-3776), the horizon and the lifecycle hook (AZ-3778) and the epoch admin (AZ-3779) on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod config;
mod cursor;
mod epoch_admin;
mod epoch_doctor;
mod epoch_resync;
mod epoch_support;
mod errors;
mod hold_limit;
mod hold_outage;
mod hold_perf;
mod hold_recheck;
mod hold_revalidate;
mod hold_support;
mod hold_wake;
mod horizon;
mod lifecycle;
mod marker_double;
mod migration;
mod probe_clock;
mod read;
mod read_concurrency;
mod read_plan;
mod read_support;
mod resync_metric;
mod rls;
mod security_injection;
mod security_revoked;
mod security_scoping;
mod stamp;
mod stamp_concurrency;
mod support;
mod system_scope_repo;
