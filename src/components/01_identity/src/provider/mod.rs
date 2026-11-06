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

/// Who the provider says the user is. `email` is set only when the provider marks it verified.
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

pub(crate) fn build(
    index: usize,
    config: &ProviderConfig,
    http: Arc<OutboundClient>,
    clock: Arc<dyn Clock>,
) -> Result<Arc<dyn ProviderClient>, IdentityError> {
    match config.kind.as_str() {
        "github" => Ok(Arc::new(github::GithubClient::new(index, config, http)?)),
        "oidc" => Ok(Arc::new(oidc::OidcClient::new(index, config, http, clock)?)),
        _ => Err(invalid(index, "kind", Reason::WrongType)),
    }
}
