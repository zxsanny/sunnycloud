//! Transfer skeleton (AZ-3733): settings, typed errors, the progress table and the `upload_sessions` and
//! `commit_receipts` migration; `start_upload` (AZ-3734). All on a real PostgreSQL 18 (`DATABASE_URL`, a
//! superuser URL).

mod access;
mod config;
mod errors;
mod migration;
mod progress;
mod rls;
mod start_open;
mod start_ports;
mod start_refusals;
mod start_shortcut;
mod start_support;
mod support;
