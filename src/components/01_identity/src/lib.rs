//! Identity: sign-in, sessions, device grants, invites and the setup token (component 01).
//!
//! This crate holds the typed configuration section, its error type, the [`IdentityAuthenticator`]
//! (`Authenticator` and `PrincipalRevalidator`), [`IdentitySignIn`] (provider sign-in: `begin` and `complete`
//! with the GitHub and generic OIDC [`ProviderClient`]s, account creation from an invite over the hook list of
//! `register_hooks`, and the mobile code exchange), [`IdentitySessions`]
//! (`SessionService`: refresh rotation, revocation, list, touch), [`IdentityDeviceGrants`]
//! (`DeviceGrantService`: the RFC 8628 device grant, the device list and revocation, over a [`GeoLookup`],
//! production: [`MaxmindGeoLookup`]), [`IdentityMaintenance`] (the `Janitor`) and [`IdentityAccounts`]
//! (`AccountAdmin` and `AccountGuard`); the other ports of `contracts` are implemented by later tasks. Wrong
//! device-approval codes count on the shared `sc_ratelimit::AbuseLimiter`, and the identity audit events go
//! through `sc_audit::AuditWriter` (`audit_events`).
//! The schema is `src/migrations/*_create_identity_tables.*.sql`.

mod abuse;
mod account_repo;
mod accounts;
mod audit_events;
mod auth_repo;
mod authenticator;
mod config;
mod device_grant_repo;
mod device_grants;
mod error;
mod geo;
mod invite_repo;
mod janitor_repo;
mod login_code_repo;
mod maintenance;
mod outbound;
mod provider;
mod scope;
mod session_cache;
mod session_repo;
mod session_service;
mod sign_in;
mod successor;

pub use accounts::IdentityAccounts;
pub use authenticator::IdentityAuthenticator;
pub use config::{IdentityConfig, ProviderConfig};
pub use device_grants::IdentityDeviceGrants;
pub use error::{HooksAlreadyRegistered, IdentityError};
pub use geo::{GeoLookup, MaxmindGeoLookup};
pub use maintenance::{IdentityMaintenance, Swept};
pub use provider::{
    AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity,
};
pub use session_service::IdentitySessions;
pub use sign_in::IdentitySignIn;
