//! Provider clients: one per provider kind behind [`ProviderClient`], so a further kind (Sign in with Apple)
//! is a new implementation and does not change the trait.

mod github;
mod oidc;

use std::error::Error;
use std::sync::Arc;

use contracts::async_trait;
use sc_clock::Clock;
use sc_config::{KeyProblem, Reason};
use thiserror::Error;

use crate::config::ProviderConfig;
use crate::error::IdentityError;
use crate::outbound::OutboundClient;

/// What the browser is sent to the provider with. The state, nonce and PKCE verifier are random values the
/// caller stores hashed or sealed; the client derives the PKCE challenge from the verifier.
pub struct AuthorizationRequest<'a> {
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub pkce_verifier: &'a str,
}

/// The callback's one-time code and the values it must be redeemed with.
pub struct CodeExchange<'a> {
    pub code: &'a str,
    pub redirect_uri: &'a str,
    pub nonce: &'a str,
    pub pkce_verifier: &'a str,
}

/// Who the provider says the user is. `email` is set only when the client was built to read emails (an
/// `email_allowlist` is configured) and the provider marks the address verified; a relay address is no email.
/// `display_name` is the provider's name claim, else its username.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIdentity {
    pub provider_user_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    /// No usable answer: unreachable, refused destination, redirect, oversized or unreadable transport.
    #[error("provider unavailable")]
    Unavailable(#[source] Box<dyn Error + Send + Sync>),
    /// The provider answered and the answer is not acceptable: an error response, a malformed answer, or a
    /// failed id-token check (signature, issuer, audience, nonce, expiry).
    #[error("provider answer rejected")]
    Rejected(#[source] Box<dyn Error + Send + Sync>),
}

#[async_trait]
pub trait ProviderClient: Send + Sync {
    /// The provider URL the browser is redirected to.
    async fn authorization_url(
        &self,
        request: &AuthorizationRequest<'_>,
    ) -> Result<String, ProviderError>;

    /// Redeems the callback code and returns the validated identity of the user.
    async fn identity(
        &self,
        exchange: &CodeExchange<'_>,
    ) -> Result<ProviderIdentity, ProviderError>;
}

fn invalid(index: usize, field: &str, reason: Reason) -> IdentityError {
    IdentityError::Config(vec![KeyProblem::new(
        format!("providers[{index}].{field}"),
        reason,
    )])
}

/// `read_email` is false when no `email_allowlist` is configured: the client then reads no email at all.
pub(crate) fn build(
    index: usize,
    config: &ProviderConfig,
    http: Arc<OutboundClient>,
    clock: Arc<dyn Clock>,
    read_email: bool,
) -> Result<Arc<dyn ProviderClient>, IdentityError> {
    match config.kind.as_str() {
        "github" => Ok(Arc::new(github::GithubClient::new(
            index, config, http, read_email,
        )?)),
        "oidc" => Ok(Arc::new(oidc::OidcClient::new(
            index, config, http, clock, read_email,
        )?)),
        _ => Err(invalid(index, "kind", Reason::WrongType)),
    }
}

/// The domain of Sign in with Apple's private relay: the address forwards mail and identifies nobody, so it is
/// treated as no email (01 §5, AC-55).
const RELAY_DOMAINS: [&str; 1] = ["privaterelay.appleid.com"];

/// Whether `address` is a relay address.
fn is_relay(address: &str) -> bool {
    address.rsplit_once('@').is_some_and(|(_, domain)| {
        RELAY_DOMAINS
            .iter()
            .any(|relay| domain.eq_ignore_ascii_case(relay))
    })
}

/// The provider's name claim, else its username; a blank value counts as absent.
fn display_name(name: Option<&str>, username: Option<&str>) -> Option<String> {
    [name, username]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relay_domain_in_any_case_is_a_relay_address_and_others_are_not() {
        // Arrange
        let addresses = [
            "x1@privaterelay.appleid.com",
            "x1@PrivateRelay.AppleID.com",
            "victim@example.test",
            "privaterelay.appleid.com@example.test",
            "no-at-sign",
        ];

        // Act
        let results = addresses.map(is_relay);

        // Assert
        assert_eq!(results, [true, true, false, false, false]);
    }

    #[test]
    fn display_name_is_the_name_else_the_username_else_none() {
        // Arrange
        let cases = [
            (Some("Ada"), Some("ada")),
            (None, Some("ada")),
            (Some("  "), Some("ada")),
            (None, None),
            (Some(""), Some(" ")),
        ];

        // Act
        let results = cases.map(|(name, username)| display_name(name, username));

        // Assert
        assert_eq!(
            results,
            [
                Some("Ada".to_owned()),
                Some("ada".to_owned()),
                Some("ada".to_owned()),
                None,
                None
            ]
        );
    }
}
