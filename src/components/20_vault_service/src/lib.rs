//! 20 vault service (v1.2): the server side of the opt-in client-side vault. Depends on `contracts` and the
//! helpers only.
//!
//! This crate holds the typed configuration section; the service and the repository are implemented by later
//! tasks. The schema is `src/migrations/*_create_vault_table.*.sql`: `vaults`, one row per account.

mod config;

pub use config::{DEFAULT_MAX_VAULT_HEADER_BYTES, VaultConfig};
