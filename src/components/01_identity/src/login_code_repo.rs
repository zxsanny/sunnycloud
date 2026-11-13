//! The statements of the mobile one-time login code. `insert` runs in the account scope of the sign-in; the
//! exchange knows only the code, so `exchange` runs on the system-scope transaction opened by `auth_repo`.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::{TokenHash, ct_eq};
use uuid::Uuid;

use crate::account_repo;
use crate::session_repo::{self, NewSession};

const ACTIVE: &str = "active";

pub(crate) struct NewLoginCode<'a> {
    pub code_hash: &'a TokenHash,
    pub account_id: Uuid,
    pub code_challenge: &'a str,
    pub client_kind: &'a str,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// What a presented code and verifier ask for: the session to mint if the code is good.
pub(crate) struct Attempt<'a> {
    /// The S256 challenge computed from the presented verifier.
    pub presented_challenge: &'a str,
    pub now: Timestamp,
    pub session_id: Uuid,
    pub token_hash: TokenHash,
    pub access_expires_at: Timestamp,
    pub refresh_hash: TokenHash,
    pub refresh_expires_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exchange {
    Minted,
    /// Unknown, reused, expired, wrong verifier or of an account that is not active: one outcome. The code
    /// is consumed in every case where it existed.
    Invalid,
}

const INSERT_CODE: &str = "INSERT INTO login_codes (code_hash, account_id, code_challenge, client_kind, \
    created_at, expires_at) VALUES ($1, $2, $3, $4, \
    'epoch'::timestamptz + $5::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $6::bigint * interval '1 microsecond')";

// Expiry is returned and decided in code: an expired code is consumed like a live one and fails the same way.
const CONSUME_CODE: &str = "DELETE FROM login_codes WHERE code_hash = $1 \
    RETURNING account_id, code_challenge, client_kind, (extract(epoch FROM expires_at) * 1000000)::bigint";

const DELETE_FOR_ACCOUNT: &str = "DELETE FROM login_codes WHERE account_id = $1";

/// Deletes the unexchanged codes of `account`, so none of them mints a session later.
pub(crate) async fn delete_for_account(tx: &mut DbTx, account: Uuid) -> Result<(), DbError> {
    sqlx::query(DELETE_FOR_ACCOUNT)
        .bind(account)
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

pub(crate) async fn insert(tx: &mut DbTx, code: &NewLoginCode<'_>) -> Result<(), DbError> {
    sqlx::query(INSERT_CODE)
        .bind(code.code_hash.as_bytes().as_slice())
        .bind(code.account_id)
        .bind(code.code_challenge)
        .bind(code.client_kind)
        .bind(code.created_at.as_unix_micros())
        .bind(code.expires_at.as_unix_micros())
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// Consumes the code (`DELETE ... RETURNING`, so concurrent exchanges of one code see one winner), checks it
/// and mints the session in the same transaction. The consumption commits whatever the outcome.
pub(crate) async fn exchange(
    tx: &mut DbTx,
    code_hash: &TokenHash,
    attempt: &Attempt<'_>,
) -> Result<Exchange, DbError> {
    let consumed = sqlx::query_as::<_, (Uuid, String, String, i64)>(CONSUME_CODE)
        .bind(code_hash.as_bytes().as_slice())
        .fetch_optional(tx.conn())
        .await?;
    let Some((account, challenge, client_kind, expires_at_micros)) = consumed else {
        return Ok(Exchange::Invalid);
    };
    if expires_at_micros <= attempt.now.as_unix_micros()
        || !ct_eq(challenge.as_bytes(), attempt.presented_challenge.as_bytes())
    {
        return Ok(Exchange::Invalid);
    }
    if account_repo::state_shared(tx, account).await?.as_deref() != Some(ACTIVE) {
        return Ok(Exchange::Invalid);
    }
    let session = NewSession {
        id: attempt.session_id,
        client_kind: &client_kind,
        label: &client_kind,
        token_hash: attempt.token_hash,
        access_expires_at: attempt.access_expires_at,
        refresh_hash: attempt.refresh_hash,
        refresh_expires_at: attempt.refresh_expires_at,
        created_at: attempt.now,
    };
    session_repo::insert(tx, account, &session).await?;
    Ok(Exchange::Minted)
}
