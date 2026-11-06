//! Transfer skeleton (AZ-3733): settings, typed errors, the progress table and the `upload_sessions` and
//! `commit_receipts` migration on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod access;
mod config;
mod errors;
mod migration;
mod progress;
mod rls;
mod support;
