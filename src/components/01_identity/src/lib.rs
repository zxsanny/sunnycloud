//! Identity: sign-in, sessions, device grants, invites and the setup token (component 01).
//!
//! This crate holds the typed configuration section, its error type and the [`IdentityAuthenticator`]
//! (`Authenticator` and `PrincipalRevalidator`); the other ports of `contracts` are implemented by later
//! tasks. The schema is `src/migrations/*_create_identity_tables.*.sql`.

mod auth_repo;
mod authenticator;
mod config;
mod error;
mod scope;
mod session_cache;

pub use authenticator::IdentityAuthenticator;
pub use config::{IdentityConfig, ProviderConfig};
pub use error::IdentityError;
