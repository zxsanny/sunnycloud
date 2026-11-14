//! The visitor's password check (AZ-3699): the token lookup of `open` first; a token that is not a live password
//! link answers `NotFound` before any permit or hash, so a guess against a token that does not exist costs one
//! indexed read. A live password link reads its PHC string in the owner's scope, takes one of the
//! `share_password_max_concurrency` permits, verifies on the blocking pool and answers a grant or `WrongPassword`.
//! The wrong-password counter and `RateLimited` are E-16/c07's.

use std::net::IpAddr;

use contracts::{AccessGrant, PublicShareError};
use sc_db::Scope;
use sc_ids::{RawToken, sha256_token};

use crate::error::SharingError;
use crate::lookup_repo;
use crate::public::{live_link, run_unexpected};
use crate::service::PgShareService;
use crate::share_repo;

impl PgShareService {
    /// `PublicShareService::verify_password` (the port is implemented once its other operations exist): an
    /// `AccessGrant` for this link expiring `share_grant_ttl` from now for the right password, `WrongPassword` for
    /// any other, `NotFound` for every dead link and for a live link without a password (owner decision
    /// 2026-11-14). `client_ip` is for the invalid-token limit (E-16/c07). Neither the password nor the grant is
    /// logged.
    pub async fn verify_password(
        &self,
        token: &RawToken,
        password: &str,
        _client_ip: IpAddr,
    ) -> Result<AccessGrant, PublicShareError> {
        let token_hash = sha256_token(token.expose());
        let row = lookup_repo::by_token_hash(&self.db, &token_hash)
            .await
            .map_err(run_unexpected)?;
        let Some(link) = live_link(row, self.clock.now_utc()).filter(|link| link.has_password)
        else {
            return Err(PublicShareError::NotFound);
        };
        let phc = self
            .db
            .run(Scope::Account(link.account_id), async |tx| {
                share_repo::password_hash(tx, link.account_id, link.id).await
            })
            .await
            .map_err(run_unexpected)?;
        // Revoked between the two reads.
        let Some(phc) = phc else {
            return Err(PublicShareError::NotFound);
        };
        if !self
            .check_password_off_worker(phc, password)
            .await
            .map_err(unexpected)?
        {
            return Err(PublicShareError::WrongPassword);
        }
        let expires_at = self
            .clock
            .now_utc()
            .checked_add(self.grant_ttl)
            .ok_or_else(|| unexpected(SharingError::TimeOutOfRange))?;
        Ok(self.grants.issue(&token_hash, expires_at))
    }
}

fn unexpected(error: SharingError) -> PublicShareError {
    PublicShareError::Unexpected(Box::new(error))
}
