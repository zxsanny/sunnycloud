//! Sign in with Apple: authorization code flow as a confidential client. Authorization asks for no scope and
//! sends no PKCE challenge (Apple documents neither, and a scope would force a `form_post` callback); the code
//! exchange is `client_secret_post` with an ES256 client-secret JWT built from the `.p8` key (see
//! `apple_secret`); the id token is checked with the machinery of `openid` against Apple's key set. The identity
//! is the `sub` claim, stable per Team ID; a relay or unverified address is no email.

use std::sync::Arc;

use contracts::async_trait;
use oauth2::AuthType;
use openidconnect::core::CoreClient;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, IssuerUrl, RedirectUrl, RequestTokenError,
};
use reqwest::Url;
use sc_clock::Clock;
use sc_config::Reason;

use super::apple_secret::ClientSecretSigner;
use super::openid;
use super::{
    AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity, invalid,
};
use crate::config::ProviderConfig;
use crate::error::IdentityError;
use crate::outbound::OutboundClient;

const ISSUER: &str = "https://appleid.apple.com";

pub(crate) struct AppleClient {
    http: Arc<OutboundClient>,
    clock: Arc<dyn Clock>,
    issuer: IssuerUrl,
    client_id: String,
    signer: ClientSecretSigner,
    read_email: bool,
}

impl AppleClient {
    /// Refuses a configuration that lacks the Team ID, the key id or a parseable `.p8` key, naming the entry.
    pub(crate) fn new(
        index: usize,
        config: &ProviderConfig,
        http: Arc<OutboundClient>,
        clock: Arc<dyn Clock>,
        read_email: bool,
    ) -> Result<Self, IdentityError> {
        let team_id = config
            .team_id
            .as_deref()
            .ok_or_else(|| invalid(index, "team_id", Reason::MissingRequired))?;
        let key_id = config
            .key_id
            .as_deref()
            .ok_or_else(|| invalid(index, "key_id", Reason::MissingRequired))?;
        let signer = ClientSecretSigner::new(
            config.client_secret.expose(),
            team_id,
            key_id,
            &config.client_id,
        )
        .map_err(|_| invalid(index, "client_secret", Reason::WrongType))?;
        let issuer = config.issuer.as_deref().unwrap_or(ISSUER);
        Ok(AppleClient {
            http,
            clock,
            issuer: IssuerUrl::new(issuer.to_owned())
                .map_err(|_| invalid(index, "issuer", Reason::WrongType))?,
            client_id: config.client_id.clone(),
            signer,
            read_email,
        })
    }
}

/// The browser's authorization URL: the authorization endpoint with the query Apple documents for a web
/// client that asks for no scope. No `scope` and no `code_challenge`, whatever the caller holds.
fn authorize_url(endpoint: &Url, client_id: &str, request: &AuthorizationRequest<'_>) -> Url {
    let mut url = endpoint.clone();
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", request.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("response_mode", "query")
        .append_pair("state", request.state)
        .append_pair("nonce", request.nonce);
    url
}

#[async_trait]
impl ProviderClient for AppleClient {
    async fn authorization_url(
        &self,
        request: &AuthorizationRequest<'_>,
    ) -> Result<String, ProviderError> {
        let metadata = openid::discover(&self.http, &self.issuer).await?;
        let url = authorize_url(
            metadata.authorization_endpoint().url(),
            &self.client_id,
            request,
        );
        Ok(url.into())
    }

    async fn identity(
        &self,
        exchange: &CodeExchange<'_>,
    ) -> Result<ProviderIdentity, ProviderError> {
        let metadata = openid::discover(&self.http, &self.issuer).await?;
        let redirect = RedirectUrl::new(exchange.redirect_uri.to_owned())
            .map_err(|error| ProviderError::Unavailable(Box::new(error)))?;
        let issued_at = self.clock.now_utc().as_unix_micros().div_euclid(1_000_000);
        let secret = self
            .signer
            .sign(issued_at)
            .map_err(|error| ProviderError::Unavailable(Box::new(error)))?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.client_id.clone()),
            Some(ClientSecret::new(secret)),
        )
        .set_redirect_uri(redirect)
        .set_auth_type(AuthType::RequestBody);
        let token = client
            .exchange_code(AuthorizationCode::new(exchange.code.to_owned()))
            .map_err(|error| ProviderError::Rejected(Box::new(error)))?
            .request_async(&*self.http)
            .await
            .map_err(|error| match error {
                RequestTokenError::Request(transport) => openid::transport_error(transport),
                other => ProviderError::Rejected(Box::new(other)),
            })?;
        openid::validated_identity(
            &client,
            &token,
            exchange.nonce,
            self.clock.as_ref(),
            self.read_email,
        )
    }

    async fn probe(&self) -> Result<(), ProviderError> {
        openid::discover(&self.http, &self.issuer).await.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authorization_url_has_no_scope_and_no_challenge_and_the_documented_query() {
        // Arrange
        let endpoint = Url::parse("https://appleid.apple.com/auth/authorize").expect("endpoint");
        let request = AuthorizationRequest {
            redirect_uri: "https://sunnycloud.test/api/v1/auth/providers/apple/callback",
            state: "state-1",
            nonce: "nonce-1",
            pkce_verifier: "verifier-the-provider-never-sees",
        };

        // Act
        let url = authorize_url(&endpoint, "test.services.id", &request);

        // Assert
        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        assert_eq!(
            query,
            vec![
                pair("client_id", "test.services.id"),
                pair(
                    "redirect_uri",
                    "https://sunnycloud.test/api/v1/auth/providers/apple/callback"
                ),
                pair("response_type", "code"),
                pair("response_mode", "query"),
                pair("state", "state-1"),
                pair("nonce", "nonce-1"),
            ]
        );
        assert!(!url.as_str().contains("verifier"));
        assert_eq!(url.path(), "/auth/authorize");
    }
}
