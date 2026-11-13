//! Vault skeleton (AZ-3922): the `vaults` migration on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL),
//! row-level security under the real roles, and the vault settings. The account lifecycle hook (AZ-3925). The
//! header service (AZ-3923) with the real 02 `TreeReader`.

mod config;
mod harness;
mod lifecycle;
mod rls;
mod schema;
mod service_header;
mod service_purge;
mod service_race;
mod service_root;
mod service_support;
mod settings;
