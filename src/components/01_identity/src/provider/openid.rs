//! The OpenID Connect machinery the `oidc` and `apple` clients share: discovery of the issuer's metadata and
//! key set, and the id-token checks of `openidconnect` (signature against the issuer's JWKS, issuer, audience,
//! nonce, expiry against the injected clock) mapped to a [`ProviderIdentity`]. The identity is the `sub` claim.

use chrono::{DateTime, Utc};
use oauth2::{EndpointMaybeSet, EndpointNotSet, EndpointSet};
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreTokenResponse};
use openidconnect::{DiscoveryError, IssuerUrl, Nonce, TokenResponse};
use sc_clock::Clock;
use thiserror::Error;

use super::{ProviderError, ProviderIdentity, display_name, is_relay};
use crate::outbound::{OutboundClient, OutboundError};

pub(super) type Discovered = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

#[derive(Debug, Error)]
#[error("the token response carries no id token")]
struct MissingIdToken;

/// Fetches the issuer's discovery document and key set. Nothing is cached: the next call asks again.
pub(super) async fn discover(
    http: &OutboundClient,
    issuer: &IssuerUrl,
) -> Result<CoreProviderMetadata, ProviderError> {
    CoreProviderMetadata::discover_async(issuer.clone(), http)
        .await
        .map_err(discovery_error)
}

/// Validates the id token of `token` against the keys `client` holds and maps its claims. `read_email` is false
/// when no allowlist is configured; the email is then not read at all.
pub(super) fn validated_identity(
    client: &Discovered,
    token: &CoreTokenResponse,
    nonce: &str,
    clock: &dyn Clock,
    read_email: bool,
) -> Result<ProviderIdentity, ProviderError> {
    let id_token = token
        .id_token()
        .ok_or_else(|| ProviderError::Rejected(Box::new(MissingIdToken)))?;
    let verifier = client
        .id_token_verifier()
        .set_time_fn(move || utc_now(clock));
    let claims = id_token
        .claims(&verifier, &Nonce::new(nonce.to_owned()))
        .map_err(|error| ProviderError::Rejected(Box::new(error)))?;
    let email = if read_email {
        verified_email(
            claims.email().map(|email| email.as_str()),
            claims.email_verified(),
        )
    } else {
        None
    };
    Ok(ProviderIdentity {
        provider_user_id: claims.subject().as_str().to_owned(),
        email,
        display_name: display_name(
            claims
                .name()
                .and_then(|name| name.get(None))
                .map(|name| name.as_str()),
            claims.preferred_username().map(|name| name.as_str()),
        ),
    })
}

fn utc_now(clock: &dyn Clock) -> DateTime<Utc> {
    // A clock outside chrono's range fails closed: the largest instant makes every token expired.
    DateTime::<Utc>::from_timestamp_micros(clock.now_utc().as_unix_micros())
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// The `email` claim, only when `email_verified` is `true` and the address is not a relay address.
fn verified_email(email: Option<&str>, email_verified: Option<bool>) -> Option<String> {
    email
        .filter(|address| email_verified == Some(true) && !is_relay(address))
        .map(str::to_owned)
}

pub(super) fn transport_error(error: OutboundError) -> ProviderError {
    ProviderError::Unavailable(Box::new(error))
}

pub(super) fn discovery_error(error: DiscoveryError<OutboundError>) -> ProviderError {
    match error {
        DiscoveryError::Request(transport) => transport_error(transport),
        other => ProviderError::Rejected(Box::new(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_verified_non_relay_email_claim_is_an_email() {
        // Arrange
        let cases = [
            (Some("victim@example.test"), Some(true)),
            (Some("victim@example.test"), Some(false)),
            (Some("victim@example.test"), None),
            (Some("x1@privaterelay.appleid.com"), Some(true)),
            (None, Some(true)),
        ];

        // Act
        let results = cases.map(|(email, verified)| verified_email(email, verified));

        // Assert
        assert_eq!(
            results,
            [
                Some("victim@example.test".to_owned()),
                None,
                None,
                None,
                None
            ]
        );
    }
}
