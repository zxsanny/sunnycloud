//! Generic OpenID Connect (Google included): discovery from the configured issuer, authorization code with
//! PKCE, and the id-token checks of `openidconnect` (see `openid`). The identity is the `sub` claim.

use std::sync::Arc;

use contracts::async_trait;
use oauth2::PkceCodeChallenge;
use openidconnect::core::{CoreClient, CoreResponseType};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    PkceCodeVerifier, RedirectUrl, RequestTokenError, Scope,
};
use sc_clock::Clock;
use sc_config::Reason;

use super::openid::{self, Discovered};
use super::{
    AuthorizationRequest, CodeExchange, ProviderClient, ProviderError, ProviderIdentity, invalid,
};
use crate::config::ProviderConfig;
use crate::error::IdentityError;
use crate::outbound::OutboundClient;

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

    async fn discover(&self, redirect_uri: &str) -> Result<Discovered, ProviderError> {
        let metadata = openid::discover(&self.http, &self.issuer).await?;
        let redirect = RedirectUrl::new(redirect_uri.to_owned())
            .map_err(|error| ProviderError::Unavailable(Box::new(error)))?;
        Ok(CoreClient::from_provider_metadata(
            metadata,
            self.client_id.clone(),
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(redirect))
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
