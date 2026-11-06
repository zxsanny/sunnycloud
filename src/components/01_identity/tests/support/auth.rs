//! Helpers of the authenticator tests: the component under test on the harness, and credentials from text.

use std::time::Duration;

use contracts::Credential;
use sc_ids::{RawToken, TokenKind};
use sunnycloud_identity::IdentityAuthenticator;

use super::Harness;

/// One day after [`super::START`]: a session access token that outlives every test.
pub const FAR_FUTURE: i64 = super::START.as_unix_micros() + 86_400_000_000;

pub fn authenticator(harness: &Harness) -> IdentityAuthenticator {
    IdentityAuthenticator::new(harness.api.clone(), harness.clock.clone(), &harness.config)
}

/// The authenticator with `session_cache_ttl` set to `ttl`.
pub fn authenticator_with_ttl(harness: &mut Harness, ttl: Duration) -> IdentityAuthenticator {
    harness.config.session_cache_ttl = ttl;
    authenticator(harness)
}

pub fn session(token: &str) -> Credential {
    Credential::Session(RawToken::from_presented(
        TokenKind::Session,
        token.to_owned(),
    ))
}

pub fn device(token: &str) -> Credential {
    Credential::Device(RawToken::from_presented(
        TokenKind::DeviceToken,
        token.to_owned(),
    ))
}
