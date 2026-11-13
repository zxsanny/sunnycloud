//! The public sign-in methods with the AC-20 counters per client address (owner decisions 2026-11-13):
//!
//! - failed sign-in steps (`SigninIp`): checked before `begin`, `complete` and `complete_link`; every one of them
//!   that ends without its result counts (bad or expired state, provider errors, `InvitationRequired`, a refused
//!   request), except `RateLimited` itself and `Unexpected` (a server fault is not the caller's attempt);
//! - invalid tokens presented at sign-in (`InvalidTokenIp`): checked before `begin` with an invite or setup token,
//!   before redeeming an invite at `complete`, and before `exchange_mobile_code`; an invite that does not redeem
//!   and a mobile code that does not exchange count.
//!
//! Each exhaustion writes one `abuse_trip` event and one WARN line (`AbuseCounter`).

use std::net::IpAddr;

use contracts::{
    AuthorizationRedirect, BeginSignIn, BrowserBinding, CallbackParams, Linked, Principal,
    SessionTokens, SignInError, SignInOutcome,
};
use sc_ids::RawToken;
use sc_ratelimit::{LimitId, Subject};

use super::IdentitySignIn;
use crate::auth_repo::KnownIdentity;
use crate::provider::ProviderIdentity;

fn step(ip: IpAddr) -> [(LimitId, Subject); 1] {
    [(LimitId::SigninIp, Subject::Ip(ip))]
}

fn token(ip: IpAddr) -> [(LimitId, Subject); 1] {
    [(LimitId::InvalidTokenIp, Subject::Ip(ip))]
}

/// Whether a failure is the caller's failed step.
fn is_failed_step(error: &SignInError) -> bool {
    !matches!(
        error,
        SignInError::RateLimited { .. } | SignInError::Unexpected(_)
    )
}

impl IdentitySignIn {
    pub async fn begin(&self, request: BeginSignIn) -> Result<AuthorizationRedirect, SignInError> {
        let ip = request.client_ip.to_canonical();
        self.allow(&step(ip))?;
        if request.invite_token.is_some() || request.setup_token.is_some() {
            self.allow(&token(ip))?;
        }
        let result = self.begin_step(request).await;
        self.count_step(&result, ip).await;
        result
    }

    pub async fn complete(
        &self,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
        client_ip: IpAddr,
    ) -> Result<SignInOutcome, SignInError> {
        let ip = client_ip.to_canonical();
        self.allow(&step(ip))?;
        let result = self
            .complete_step(provider, callback_params, browser_binding, host, ip)
            .await;
        self.count_step(&result, ip).await;
        result
    }

    pub async fn complete_link(
        &self,
        principal: &Principal,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
        client_ip: IpAddr,
    ) -> Result<Linked, SignInError> {
        let ip = client_ip.to_canonical();
        self.allow(&step(ip))?;
        let result = self
            .complete_link_step(principal, provider, callback_params, browser_binding, host)
            .await;
        self.count_step(&result, ip).await;
        result
    }

    pub async fn exchange_mobile_code(
        &self,
        code: &RawToken,
        code_verifier: &str,
        client_ip: IpAddr,
    ) -> Result<SessionTokens, SignInError> {
        let ip = client_ip.to_canonical();
        self.allow(&token(ip))?;
        let result = self.exchange_step(code, code_verifier).await;
        if let Err(SignInError::InvalidCode) = &result {
            self.limits.record(&token(ip), None, ip).await;
        }
        result
    }

    /// Account creation from the invite presented at `begin`, with that invite counted as a token.
    pub(super) async fn register_counted(
        &self,
        provider: &str,
        identity: &ProviderIdentity,
        invite_token_hash: Option<&[u8]>,
        ip: IpAddr,
    ) -> Result<KnownIdentity, SignInError> {
        if invite_token_hash.is_some() {
            self.allow(&token(ip))?;
        }
        let result = self.register(provider, identity, invite_token_hash).await;
        if invite_token_hash.is_some() && matches!(result, Err(SignInError::InvitationRequired)) {
            self.limits.record(&token(ip), None, ip).await;
        }
        result
    }

    fn allow(&self, dimensions: &[(LimitId, Subject)]) -> Result<(), SignInError> {
        match self.limits.retry_after(dimensions) {
            Some(retry_after) => Err(SignInError::RateLimited { retry_after }),
            None => Ok(()),
        }
    }

    async fn count_step<T>(&self, result: &Result<T, SignInError>, ip: IpAddr) {
        if let Err(error) = result
            && is_failed_step(error)
        {
            self.limits.record(&step(ip), None, ip).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sc_error::ErrorCode;

    use super::*;

    #[test]
    fn every_failure_but_a_limit_or_a_server_fault_is_a_failed_step() {
        // Arrange
        let counted = [
            SignInError::UntrustedState,
            SignInError::InvitationRequired,
            SignInError::ProviderUnavailable,
            SignInError::TrustedFailure {
                code: ErrorCode::Unauthenticated,
            },
            SignInError::HostMismatch,
            SignInError::IdentityTaken,
        ];
        let not_counted = [
            SignInError::RateLimited {
                retry_after: Duration::from_secs(1),
            },
            SignInError::Unexpected(Box::new(std::io::Error::other("db"))),
        ];

        // Act
        let counted: Vec<bool> = counted.iter().map(is_failed_step).collect();
        let not_counted: Vec<bool> = not_counted.iter().map(is_failed_step).collect();

        // Assert
        assert_eq!(counted, vec![true; 6]);
        assert_eq!(not_counted, vec![false; 2]);
    }
}
