//! Identity: sign-in, sessions, device grants, invites and the setup token (component 01).
//!
//! This crate holds the typed configuration section, its error type, the [`IdentityAuthenticator`]
//! (`Authenticator` and `PrincipalRevalidator`) and [`IdentitySignIn`] (provider sign-in: `begin` and
//! `complete` with the GitHub and generic OIDC [`ProviderClient`]s); the other ports of `contracts` are
//! implemented by later tasks. The schema is `src/migrations/*_create_identity_tables.*.sql`.

mod auth_repo;
mod authenticator;
mod config;
mod error;
mod outbound;
mod provider;
mod scope;
mod session_cache;
mod sign_in;

pub use authenticator::IdentityAuthenticator;
pub use config::{IdentityConfig, ProviderConfig};
pub use error::IdentityError;
pub use provider::{
    AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity,
};
pub use sign_in::IdentitySignIn;
