//! 20 vault service (v1.2): the server side of the opt-in client-side vault. Depends on `contracts` and the
//! helpers only; it never links `vault-core` and never parses the header.
//!
//! This crate holds the typed configuration section, the header service (`PgVaultService`: create, read and
//! replace with a SHA-256 ETag) and the account lifecycle hook. The schema is
//! `src/migrations/*_create_vault_table.*.sql`: `vaults`, one row per account, removed with its root folder when
//! 02 purges it (`*_cascade_vault_on_root_purge.*.sql`).

mod config;
mod lifecycle;
mod service;
mod vault_repo;

pub use config::{DEFAULT_MAX_VAULT_HEADER_BYTES, VaultConfig};
pub use lifecycle::PgVaultLifecycle;
pub use service::PgVaultService;
