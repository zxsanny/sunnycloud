//! Vault skeleton (AZ-3922): the `vaults` migration on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL),
//! row-level security under the real roles, and the vault settings. The account lifecycle hook (AZ-3925). The
//! header service (AZ-3923) with the real 02 `TreeReader`. The unlock-failure report (AZ-3924) with the real
//! `sc_audit` writer.

mod config;
mod harness;
mod lifecycle;
mod logs;
mod rls;
mod schema;
mod service_header;
mod service_purge;
mod service_race;
mod service_root;
mod service_support;
mod service_unlock_failure;
mod settings;
