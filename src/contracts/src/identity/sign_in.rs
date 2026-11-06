use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use sc_error::ErrorCode;
use sc_ids::RawToken;
use thiserror::Error;

use super::model::{ClientKind, Principal, ProviderInfo, SessionTokens};
use crate::common::Fault;

/// Value of the browser-binding cookie; hashed together with the OAuth state, so it prints a placeholder.
pub struct BrowserBinding(pub String);

impl fmt::Debug for BrowserBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrowserBinding(<redacted>)")
    }
}

/// Query or form parameters of the provider callback, in arrival order. `Debug` prints the count only.
pub struct CallbackParams(pub Vec<(String, String)>);

impl fmt::Debug for CallbackParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CallbackParams({} <redacted>)", self.0.len())
    }
}

#[derive(Debug)]
pub struct BeginSignIn {
    pub provider: String,
    pub return_to: String,
    pub client_kind: ClientKind,
    pub invite_token: Option<RawToken>,
    pub setup_token: Option<RawToken>,
    pub code_challenge: Option<String>,
    pub host: String,
}

#[derive(Debug)]
pub struct AuthorizationRedirect {
    pub url: String,
    pub browser_binding: BrowserBinding,
}

#[derive(Debug)]
pub enum SignInOutcome {
    Session {
        tokens: SessionTokens,
        return_to: String,
    },
    MobileCode {
        code: RawToken,
        return_to: String,
    },
}

/// A provider identity was linked to the signed-in account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Linked;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderList {
    pub providers: Vec<ProviderInfo>,
    pub setup_required: bool,
}

#[derive(Debug, Error)]
pub enum SignInError {
    #[error("unknown provider")]
    UnknownProvider,
    #[error("invalid return path")]
    InvalidReturnPath,
    #[error("host mismatch")]
    HostMismatch,
    #[error("rate limited")]
    RateLimited { retry_after: Duration },
    #[error("untrusted state")]
    UntrustedState,
    #[error("sign-in failed: {}", .code.as_str())]
    TrustedFailure { code: ErrorCode },
    #[error("invitation required")]
    InvitationRequired,
    #[error("provider unavailable")]
    ProviderUnavailable,
    #[error("identity taken")]
    IdentityTaken,
    #[error("invalid code")]
    InvalidCode,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// Provider sign-in, account linking and the mobile code exchange.
#[async_trait]
pub trait SignInService: Send + Sync {
    async fn begin(&self, request: BeginSignIn) -> Result<AuthorizationRedirect, SignInError>;

    async fn complete(
        &self,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
    ) -> Result<SignInOutcome, SignInError>;

    async fn begin_link(
        &self,
        principal: &Principal,
        provider: &str,
    ) -> Result<AuthorizationRedirect, SignInError>;

    async fn complete_link(
        &self,
        principal: &Principal,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
    ) -> Result<Linked, SignInError>;

    async fn exchange_mobile_code(
        &self,
        code: &RawToken,
        code_verifier: &str,
    ) -> Result<SessionTokens, SignInError>;

    async fn list_providers(&self) -> Result<ProviderList, SignInError>;
}
