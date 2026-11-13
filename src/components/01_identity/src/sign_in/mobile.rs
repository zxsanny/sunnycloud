//! The mobile branch of sign-in: `complete` stores a one-time login code instead of a session, and
//! `exchange_mobile_code` turns that code and the app's PKCE verifier into a normal session.

use contracts::{SessionTokens, SignInError};
use sc_db::Scope;
use sc_ids::{RawToken, TokenKind, new_id, new_token, sha256_token};

use super::rules::s256_challenge;
use super::{IdentitySignIn, after, unexpected};
use crate::account_repo;
use crate::auth_repo::{self, KnownIdentity};
use crate::login_code_repo::{self, Attempt, Exchange, NewLoginCode};

impl IdentitySignIn {
    /// Stores the hash of a new 256-bit code for the identity's account, bound to the PKCE challenge the app
    /// sent at `begin`, and counts the sign-in as the identity's last login. The code is returned once.
    pub(super) async fn store_login_code(
        &self,
        identity: &KnownIdentity,
        client_kind: &str,
        code_challenge: &str,
    ) -> Result<RawToken, SignInError> {
        let code = new_token(TokenKind::MobileLoginCode).map_err(unexpected)?;
        let created_at = self.clock.now_utc();
        let code_hash = sha256_token(code.expose());
        let row = NewLoginCode {
            code_hash: &code_hash,
            account_id: identity.account_id,
            code_challenge,
            client_kind,
            created_at,
            expires_at: after(created_at, self.login_code_ttl)?,
        };
        self.db
            .run(Scope::Account(identity.account_id), async |tx| {
                login_code_repo::insert(tx, &row).await?;
                account_repo::touch_identity(tx, identity.identity_id, created_at).await
            })
            .await
            .map_err(unexpected)?;
        Ok(code)
    }

    /// Exchanges a login code for a session in one transaction. A reused, expired, unknown or wrongly proven
    /// code, and a code of an account that is no longer active, all give `InvalidCode`.
    pub(super) async fn exchange_step(
        &self,
        code: &RawToken,
        code_verifier: &str,
    ) -> Result<SessionTokens, SignInError> {
        let access = new_token(TokenKind::Session).map_err(unexpected)?;
        let refresh = new_token(TokenKind::Refresh).map_err(unexpected)?;
        let now = self.clock.now_utc();
        let presented_challenge = s256_challenge(code_verifier);
        let attempt = Attempt {
            presented_challenge: &presented_challenge,
            now,
            session_id: new_id(self.clock.as_ref()),
            token_hash: sha256_token(access.expose()),
            access_expires_at: after(now, self.access_token_ttl)?,
            refresh_hash: sha256_token(refresh.expose()),
            refresh_expires_at: after(now, self.refresh_token_ttl)?,
        };
        match auth_repo::exchange_login_code(&self.db, &sha256_token(code.expose()), &attempt)
            .await
            .map_err(unexpected)?
        {
            Exchange::Minted => Ok(SessionTokens {
                access,
                refresh,
                expires_in: self.access_token_ttl,
            }),
            Exchange::Invalid => Err(SignInError::InvalidCode),
        }
    }
}
