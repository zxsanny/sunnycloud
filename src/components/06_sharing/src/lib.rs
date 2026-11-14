//! 06 sharing: share links for visitors without an account. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section with the grant key rule, the owner side of the links
//! (`PgShareService`: create, list, revoke) with the visitor's `open`, `verify_password` (a bounded Argon2id check
//! that issues a stateless HMAC access grant, `grant`) and `authorize_content` (one system-scope lookup by token
//! hash in `lookup_repo`, then the same target reads in every case, nothing cached), and the account erase hook
//! (`PgShareLifecycle`); derived files follow in a later task.
//! The schema is `src/migrations/*_create_share_tables.*.sql`: `share_links`, whose composite foreign key to the
//! tree's `nodes` cascades, so a purged file removes its links. Only the SHA-256 of a token and the Argon2id PHC
//! string of a password are stored.

mod config;
mod content;
mod error;
mod grant;
mod lifecycle;
mod lookup_repo;
mod password;
mod public;
mod secret;
mod service;
mod share_repo;

pub use config::{
    ARGON2_PARAMS_KEY, GRANT_KEY_KEY, GRANT_KEY_MIN_BYTES, PASSWORD_MAX_CONCURRENCY_KEY,
    ShareConfig,
};
pub use error::SharingError;
pub use lifecycle::PgShareLifecycle;
pub use secret::SHARE_TOKEN_PREFIX;
pub use service::PgShareService;
