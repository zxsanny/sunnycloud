//! The visitor's password check (AZ-3699, AZ-3701): the address is checked against the invalid-token limit, then
//! the token lookup of `open` runs; a token that is not a live password link answers `NotFound` (counted as an
//! invalid token, as on the other visitor routes) before any permit or hash, so a guess against a token that does
//! not exist costs one indexed read. A live password link then takes one token of its wrong-password bucket before
//! any password work (`RateLimited` when it is empty, even for the right password, G-06-2), reads its PHC string in
//! the owner's scope, takes one of the `share_password_max_concurrency` permits, verifies on the blocking pool and
//! answers a grant (clearing the link's counter) or `WrongPassword` (keeping the token taken).

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
    /// `PublicShareService::verify_password`: an `AccessGrant` for this link expiring `share_grant_ttl` from now for
    /// the right password, `WrongPassword` for any other, `NotFound` for every dead link and for a live link without
    /// a password (owner decision 2026-11-14), `RateLimited` while `client_ip` has used up its invalid tokens or the
    /// link its wrong passwords (AC-20). Neither the password nor the grant is logged.
    pub(crate) async fn verify_link_password(
        &self,
        token: &RawToken,
        password: &str,
        client_ip: IpAddr,
    ) -> Result<AccessGrant, PublicShareError> {
        let ip = client_ip.to_canonical();
        self.check_token_ip(ip)?;
        let token_hash = sha256_token(token.expose());
        let row = lookup_repo::by_token_hash(&self.db, &token_hash)
            .await
            .map_err(run_unexpected)?;
        let Some(link) = live_link(row, self.clock.now_utc()).filter(|link| link.has_password)
        else {
            self.count_invalid_token(ip).await;
            return Err(PublicShareError::NotFound);
        };
        let attempt = self.reserve_password_attempt(link.id)?;
        let phc = self
            .db
            .run(Scope::Account(link.account_id), async |tx| {
                share_repo::password_hash(tx, link.account_id, link.id).await
            })
            .await
            .map_err(run_unexpected)?;
        // Revoked between the two reads.
        let Some(phc) = phc else {
            self.count_invalid_token(ip).await;
            return Err(PublicShareError::NotFound);
        };
        if !self
            .check_password_off_worker(phc, password)
            .await
            .map_err(unexpected)?
        {
            self.wrong_password(attempt, ip).await;
            return Err(PublicShareError::WrongPassword);
        }
        self.right_password(attempt);
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
