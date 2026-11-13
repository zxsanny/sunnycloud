//! 06 sharing: share links for visitors without an account. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section with the grant key rule and the owner side of the links
//! (`PgShareService`: create, list, revoke); the visitor side is implemented by later tasks. The schema is
//! `src/migrations/*_create_share_tables.*.sql`: `share_links`, whose composite foreign key to the tree's `nodes`
//! cascades, so a purged file removes its links. Only the SHA-256 of a token and the Argon2id PHC string of a
//! password are stored.

mod config;
mod error;
mod secret;
mod service;
mod share_repo;

pub use config::{ARGON2_PARAMS_KEY, GRANT_KEY_KEY, GRANT_KEY_MIN_BYTES, ShareConfig};
pub use error::SharingError;
pub use secret::SHARE_TOKEN_PREFIX;
pub use service::PgShareService;
