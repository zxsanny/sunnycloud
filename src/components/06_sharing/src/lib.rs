//! 06 sharing: share links for visitors without an account. Depends on `contracts` and the helpers only.
//!
//! This crate holds the typed configuration section with the grant key rule; the services and the repository are
//! implemented by later tasks. The schema is `src/migrations/*_create_share_tables.*.sql`: `share_links`, whose
//! composite foreign key to the tree's `nodes` cascades, so a purged file removes its links.

mod config;

pub use config::{GRANT_KEY_KEY, GRANT_KEY_MIN_BYTES, ShareConfig};
