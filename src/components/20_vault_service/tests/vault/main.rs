//! Vault skeleton (AZ-3922): the `vaults` migration on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL),
//! row-level security under the real roles, and the vault settings.

mod config;
mod harness;
mod rls;
mod schema;
mod settings;
