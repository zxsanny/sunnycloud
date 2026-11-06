//! Tree skeleton (AZ-3706): settings, typed errors, the retry-once rule and the `nodes`/`revisions` migration
//! on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod config;
mod constraints;
mod errors;
mod keys;
mod migration;
mod retry;
mod rls;
mod support;
