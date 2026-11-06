//! Identity: sign-in, sessions, device grants, invites and the setup token (component 01).
//!
//! This crate holds the typed configuration section and its error type; the ports of `contracts` are
//! implemented by later tasks. The schema is `src/migrations/*_create_identity_tables.*.sql`.

mod config;
mod error;

pub use config::{IdentityConfig, ProviderConfig};
pub use error::IdentityError;
