//! `contracts::SignInService` over the inherent methods, so the composition root can hand the service to 11 as
//! `Arc<dyn SignInService>`.

use std::net::IpAddr;

use contracts::{
    AuthorizationRedirect, BeginSignIn, BrowserBinding, CallbackParams, Linked, Principal,
    ProviderList, SessionTokens, SignInError, SignInOutcome, SignInService, async_trait,
};
use sc_ids::RawToken;

use super::IdentitySignIn;

#[async_trait]
impl SignInService for IdentitySignIn {
    async fn begin(&self, request: BeginSignIn) -> Result<AuthorizationRedirect, SignInError> {
        IdentitySignIn::begin(self, request).await
    }

    async fn complete(
        &self,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
        client_ip: IpAddr,
    ) -> Result<SignInOutcome, SignInError> {
        IdentitySignIn::complete(
            self,
            provider,
            callback_params,
            browser_binding,
            host,
            client_ip,
        )
        .await
    }

    async fn begin_link(
        &self,
        principal: &Principal,
        provider: &str,
    ) -> Result<AuthorizationRedirect, SignInError> {
        IdentitySignIn::begin_link(self, principal, provider).await
    }

    async fn complete_link(
        &self,
        principal: &Principal,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
        client_ip: IpAddr,
    ) -> Result<Linked, SignInError> {
        IdentitySignIn::complete_link(
            self,
            principal,
            provider,
            callback_params,
            browser_binding,
            host,
            client_ip,
        )
        .await
    }

    async fn exchange_mobile_code(
        &self,
        code: &RawToken,
        code_verifier: &str,
        client_ip: IpAddr,
    ) -> Result<SessionTokens, SignInError> {
        IdentitySignIn::exchange_mobile_code(self, code, code_verifier, client_ip).await
    }

    async fn list_providers(&self) -> Result<ProviderList, SignInError> {
        IdentitySignIn::list_providers(self).await
    }
}
