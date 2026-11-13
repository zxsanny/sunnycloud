//! GitHub: OAuth 2 authorization code with PKCE, then the profile and the per-address verified email list.
//! GitHub is not an OIDC issuer, so the identity is the numeric user id of `GET /user`.

use std::sync::Arc;

use oauth2::basic::BasicClient;
use oauth2::http::{self, Method, header};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RequestTokenError, Scope, TokenResponse, TokenUrl,
};
use sc_config::Reason;
use serde::Deserialize;

use super::{
    AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity,
    display_name, invalid, is_relay,
};
use crate::config::ProviderConfig;
use crate::error::IdentityError;
use crate::outbound::{OutboundClient, OutboundError};

const OAUTH_BASE: &str = "https://github.com";
const API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = "sunnycloud";

pub(crate) struct GithubClient {
    http: Arc<OutboundClient>,
    client_id: ClientId,
    client_secret: ClientSecret,
    auth_url: AuthUrl,
    token_url: TokenUrl,
    api: String,
    read_email: bool,
}

#[derive(Deserialize)]
struct User {
    id: u64,
    login: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Email {
    email: String,
    primary: bool,
    verified: bool,
}

impl GithubClient {
    /// `config.issuer` replaces the base of every GitHub URL; it points the client at a test provider.
    pub(crate) fn new(
        index: usize,
        config: &ProviderConfig,
        http: Arc<OutboundClient>,
        read_email: bool,
    ) -> Result<Self, IdentityError> {
        let (oauth, api) = match &config.issuer {
            Some(base) => (base.trim_end_matches('/'), base.trim_end_matches('/')),
            None => (OAUTH_BASE, API_BASE),
        };
        let bad_issuer = |_| invalid(index, "issuer", Reason::WrongType);
        Ok(GithubClient {
            http,
            client_id: ClientId::new(config.client_id.clone()),
            client_secret: ClientSecret::new(config.client_secret.expose().clone()),
            auth_url: AuthUrl::new(format!("{oauth}/login/oauth/authorize")).map_err(bad_issuer)?,
            token_url: TokenUrl::new(format!("{oauth}/login/oauth/access_token"))
                .map_err(bad_issuer)?,
            api: api.to_owned(),
            read_email,
        })
    }

    fn oauth(
        &self,
        redirect_uri: &str,
    ) -> Result<
        BasicClient<
            oauth2::EndpointSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointSet,
        >,
        ProviderError,
    > {
        let redirect = RedirectUrl::new(redirect_uri.to_owned())
            .map_err(|error| ProviderError::Unavailable(Box::new(error)))?;
        Ok(BasicClient::new(self.client_id.clone())
            .set_client_secret(self.client_secret.clone())
            .set_auth_uri(self.auth_url.clone())
            .set_token_uri(self.token_url.clone())
            .set_redirect_uri(redirect))
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        access_token: &str,
    ) -> Result<T, ProviderError> {
        let request = http::Request::builder()
            .method(Method::GET)
            .uri(format!("{}{path}", self.api))
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .header(header::ACCEPT, "application/vnd.github+json")
            .header(header::USER_AGENT, USER_AGENT)
            .body(Vec::new())
            .map_err(|error| ProviderError::Unavailable(Box::new(error)))?;
        let response = self
            .http
            .send(request)
            .await
            .map_err(|error| ProviderError::Unavailable(Box::new(error)))?;
        let status = response.status();
        if status.is_server_error() {
            return Err(ProviderError::Unavailable(Box::new(StatusError(status))));
        }
        if !status.is_success() {
            return Err(ProviderError::Rejected(Box::new(StatusError(status))));
        }
        serde_json::from_slice(response.body())
            .map_err(|error| ProviderError::Rejected(Box::new(error)))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("provider answered {0}")]
struct StatusError(http::StatusCode);

/// The primary address, only when GitHub marks that address verified and it is not a relay address.
fn verified_primary(emails: Vec<Email>) -> Option<String> {
    emails
        .into_iter()
        .find(|entry| entry.primary)
        .filter(|entry| entry.verified && !is_relay(&entry.email))
        .map(|entry| entry.email)
}

#[contracts::async_trait]
impl ProviderClient for GithubClient {
    async fn authorization_url(
        &self,
        request: &AuthorizationRequest<'_>,
    ) -> Result<String, ProviderError> {
        let challenge = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(
            request.pkce_verifier.to_owned(),
        ));
        let state = request.state.to_owned();
        let (url, _) = self
            .oauth(request.redirect_uri)?
            .authorize_url(move || CsrfToken::new(state))
            .add_scope(Scope::new("read:user".to_owned()))
            .add_scope(Scope::new("user:email".to_owned()))
            .set_pkce_challenge(challenge)
            .url();
        Ok(url.into())
    }

    async fn identity(
        &self,
        exchange: &CodeExchange<'_>,
    ) -> Result<ProviderIdentity, ProviderError> {
        let token = self
            .oauth(exchange.redirect_uri)?
            .exchange_code(AuthorizationCode::new(exchange.code.to_owned()))
            .set_pkce_verifier(PkceCodeVerifier::new(exchange.pkce_verifier.to_owned()))
            .request_async(&*self.http)
            .await
            .map_err(token_error)?;
        let access_token = token.access_token().secret();
        let user: User = self.get("/user", access_token).await?;
        let email = if self.read_email {
            verified_primary(self.get("/user/emails", access_token).await?)
        } else {
            None
        };
        Ok(ProviderIdentity {
            provider_user_id: user.id.to_string(),
            email,
            display_name: display_name(user.name.as_deref(), Some(&user.login)),
        })
    }
}

fn token_error<E>(error: RequestTokenError<OutboundError, E>) -> ProviderError
where
    E: oauth2::ErrorResponse + Send + Sync + 'static,
{
    match error {
        RequestTokenError::Request(transport) => ProviderError::Unavailable(Box::new(transport)),
        other => ProviderError::Rejected(Box::new(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(email: &str, primary: bool, verified: bool) -> Email {
        Email {
            email: email.to_owned(),
            primary,
            verified,
        }
    }

    #[test]
    fn only_a_verified_primary_address_is_returned() {
        // Arrange
        let unverified_primary = vec![
            entry("a@x.test", true, false),
            entry("b@x.test", false, true),
        ];
        let verified_primary_second = vec![
            entry("b@x.test", false, true),
            entry("a@x.test", true, true),
        ];
        let none: Vec<Email> = Vec::new();
        let relay_primary = vec![entry("x1@privaterelay.appleid.com", true, true)];

        // Act
        let results = [
            verified_primary(unverified_primary),
            verified_primary(verified_primary_second),
            verified_primary(none),
            verified_primary(relay_primary),
        ];

        // Assert
        assert_eq!(results, [None, Some("a@x.test".to_owned()), None, None]);
    }
}
