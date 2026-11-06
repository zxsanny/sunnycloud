//! Identity: sign-in, sessions, device grants, invites and the setup token (component 01).
//!
//! This crate holds the typed configuration section, its error type, the [`IdentityAuthenticator`]
//! (`Authenticator` and `PrincipalRevalidator`) and [`IdentitySignIn`] (provider sign-in: `begin` and
//! `complete` with the GitHub and generic OIDC [`ProviderClient`]s) and [`IdentitySessions`] (`SessionService`:
//! refresh rotation, revocation, list, touch) and [`IdentityDeviceGrants`] (`DeviceGrantService`: the RFC 8628
//! device grant, the device list and revocation, over a [`GeoLookup`], production: [`MaxmindGeoLookup`]); the other ports of `contracts` are
//! implemented by later tasks. The schema is `src/migrations/*_create_identity_tables.*.sql`.

mod auth_repo;
mod authenticator;
mod config;
mod device_grant_repo;
mod device_grants;
mod error;
mod geo;
mod outbound;
mod provider;
mod scope;
mod session_cache;
mod session_repo;
mod session_service;
mod sign_in;
mod successor;

pub use authenticator::IdentityAuthenticator;
pub use config::{IdentityConfig, ProviderConfig};
pub use device_grants::IdentityDeviceGrants;
pub use error::IdentityError;
pub use geo::{GeoLookup, MaxmindGeoLookup};
pub use provider::{
    AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity,
};
pub use session_service::IdentitySessions;
pub use sign_in::IdentitySignIn;
