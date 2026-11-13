//! Generic OpenID Connect (Google included): discovery from the configured issuer, authorization code with
//! PKCE, and the id-token checks of `openidconnect` (signature against the issuer's JWKS, issuer, audience,
//! nonce, expiry against the injected clock). The identity is the `sub` claim.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use contracts::async_trait;
use oauth2::{EndpointMaybeSet, EndpointNotSet, EndpointSet, PkceCodeChallenge};
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, DiscoveryError,
    IssuerUrl, Nonce, PkceCodeVerifier, RedirectUrl, RequestTokenError, Scope, TokenResponse,
};
use sc_clock::Clock;
use sc_config::Reason;
use thiserror::Error;

use super::{
    AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity,
    display_name, invalid, is_relay,
};
use crate::config::ProviderConfig;
use crate::error::IdentityError;
use crate::outbound::{OutboundClient, OutboundError};

type Discovered = CoreClient<
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

pub(crate) struct OidcClient {
    http: Arc<OutboundClient>,
    clock: Arc<dyn Clock>,
    issuer: IssuerUrl,
    client_id: ClientId,
    client_secret: ClientSecret,
    read_email: bool,
}

impl OidcClient {
    pub(crate) fn new(
        index: usize,
        config: &ProviderConfig,
        http: Arc<OutboundClient>,
        clock: Arc<dyn Clock>,
        read_email: bool,
    ) -> Result<Self, IdentityError> {
        let issuer = config
            .issuer
            .clone()
            .ok_or_else(|| invalid(index, "issuer", Reason::MissingRequired))?;
        Ok(OidcClient {
            http,
            clock,
            issuer: IssuerUrl::new(issuer)
                .map_err(|_| invalid(index, "issuer", Reason::WrongType))?,
            client_id: ClientId::new(config.client_id.clone()),
            client_secret: ClientSecret::new(config.client_secret.expose().clone()),
            read_email,
        })
    }

    /// Fetches the issuer's discovery document and key set. Nothing is cached: the next call asks again.
    async fn discover(&self, redirect_uri: &str) -> Result<Discovered, ProviderError> {
        let metadata = CoreProviderMetadata::discover_async(self.issuer.clone(), &*self.http)
            .await
            .map_err(discovery_error)?;
        let redirect = RedirectUrl::new(redirect_uri.to_owned())
            .map_err(|error| ProviderError::Unavailable(Box::new(error)))?;
        Ok(CoreClient::from_provider_metadata(
            metadata,
            self.client_id.clone(),
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(redirect))
    }

    fn utc_now(&self) -> DateTime<Utc> {
        // A clock outside chrono's range fails closed: the largest instant makes every token expired.
        DateTime::<Utc>::from_timestamp_micros(self.clock.now_utc().as_unix_micros())
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
    }
}

#[async_trait]
impl ProviderClient for OidcClient {
    async fn authorization_url(
        &self,
        request: &AuthorizationRequest<'_>,
    ) -> Result<String, ProviderError> {
        let client = self.discover(request.redirect_uri).await?;
        let challenge = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(
            request.pkce_verifier.to_owned(),
        ));
        let (state, nonce) = (request.state.to_owned(), request.nonce.to_owned());
        let (url, _, _) = client
            .authorize_url(
                AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                move || CsrfToken::new(state),
                move || Nonce::new(nonce),
            )
            .add_scope(Scope::new("email".to_owned()))
            .add_scope(Scope::new("profile".to_owned()))
            .set_pkce_challenge(challenge)
            .url();
        Ok(url.into())
    }

    async fn identity(
        &self,
        exchange: &CodeExchange<'_>,
    ) -> Result<ProviderIdentity, ProviderError> {
        let client = self.discover(exchange.redirect_uri).await?;
        let token = client
            .exchange_code(AuthorizationCode::new(exchange.code.to_owned()))
            .map_err(|error| ProviderError::Rejected(Box::new(error)))?
            .set_pkce_verifier(PkceCodeVerifier::new(exchange.pkce_verifier.to_owned()))
            .request_async(&*self.http)
            .await
            .map_err(|error| match error {
                RequestTokenError::Request(transport) => transport_error(transport),
                other => ProviderError::Rejected(Box::new(other)),
            })?;
        let id_token = token
            .id_token()
            .ok_or_else(|| ProviderError::Rejected(Box::new(MissingIdToken)))?;
        let verifier = client
            .id_token_verifier()
            .set_time_fn(move || self.utc_now());
        let claims = id_token
            .claims(&verifier, &Nonce::new(exchange.nonce.to_owned()))
            .map_err(|error| ProviderError::Rejected(Box::new(error)))?;
        let email = if self.read_email {
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
}

/// The `email` claim, only when `email_verified` is `true` and the address is not a relay address.
fn verified_email(email: Option<&str>, email_verified: Option<bool>) -> Option<String> {
    email
        .filter(|address| email_verified == Some(true) && !is_relay(address))
        .map(str::to_owned)
}

fn transport_error(error: OutboundError) -> ProviderError {
    ProviderError::Unavailable(Box::new(error))
}

fn discovery_error(error: DiscoveryError<OutboundError>) -> ProviderError {
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
