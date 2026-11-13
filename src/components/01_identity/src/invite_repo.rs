//! The statements on invite rows (member, operator admin and re-bind invites, and the setup token, data model
//! D2). Each runs on the caller's transaction, which is in system scope: `invites` has no account and no row
//! policy. Only token hashes are stored.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::TokenHash;
use uuid::Uuid;

// Any kind: the caller decides by `is_setup` and `target_account_id` which redemption applies.
const OPEN_TOKEN: &str = "SELECT id, kind = 'setup', grants_role, target_account_id FROM invites \
    WHERE token_hash = $1 AND redeemed_at IS NULL \
    AND expires_at > 'epoch'::timestamptz + $2::bigint * interval '1 microsecond'";

// The one statement that decides a redemption race: the second of two concurrent updates waits for the first to
// commit, re-reads the row and matches nothing.
const REDEEM: &str = "UPDATE invites \
    SET redeemed_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', redeemed_by = $2 \
    WHERE id = $1 AND redeemed_at IS NULL \
    AND expires_at > 'epoch'::timestamptz + $3::bigint * interval '1 microsecond'";

const INSERT: &str = "INSERT INTO invites \
    (id, kind, token_hash, grants_role, target_account_id, created_by, note, created_at, expires_at) \
    VALUES ($1, $2, $3, $4, $5, $6, $7, \
    'epoch'::timestamptz + $8::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $9::bigint * interval '1 microsecond')";

// An unredeemed setup token, expired or not: the partial unique index allows one.
const DELETE_OPEN_SETUP: &str = "DELETE FROM invites WHERE kind = 'setup' AND redeemed_at IS NULL";

const DELETE_OPEN_REBIND: &str =
    "DELETE FROM invites WHERE target_account_id = $1 AND redeemed_at IS NULL";

// The invites an admin manages: member and operator admin invites; re-bind invites and the setup token are the
// operator's.
const LIST: &str = "SELECT id, COALESCE(note, ''), \
    (extract(epoch FROM created_at) * 1000000)::bigint, (extract(epoch FROM expires_at) * 1000000)::bigint, \
    (extract(epoch FROM redeemed_at) * 1000000)::bigint \
    FROM invites WHERE kind = 'invite' AND target_account_id IS NULL ORDER BY created_at DESC, id";

// Revocation deletes the row; a redeemed invite is history and stays.
const REVOKE: &str = "DELETE FROM invites WHERE id = $1 AND kind = 'invite' \
    AND target_account_id IS NULL AND redeemed_at IS NULL";

/// A token row that was open when read; the redemption decides whether it still is.
pub(crate) struct OpenToken {
    pub id: Uuid,
    pub is_setup: bool,
    pub grants_role: String,
    /// Set on a re-bind invite: the account the signing-in identity is bound to.
    pub target_account_id: Option<Uuid>,
}

/// The stored kind of a token row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InviteKind {
    Invite,
    Setup,
}

/// A new token row; the plaintext never reaches this module.
pub(crate) struct NewInvite<'a> {
    pub id: Uuid,
    pub kind: InviteKind,
    pub token_hash: TokenHash,
    /// `user` or `admin` (a setup token is always `admin`).
    pub grants_role: &'static str,
    pub target_account_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub note: Option<&'a str>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// A listed invite, without its token hash.
pub(crate) struct ListedInvite {
    pub id: Uuid,
    pub note: String,
    pub created_at_micros: i64,
    pub expires_at_micros: i64,
    pub redeemed_at_micros: Option<i64>,
}

type ListedTuple = (Uuid, String, i64, i64, Option<i64>);

/// The token row whose hash is `token_hash`, when it is unredeemed and not expired at `now`.
pub(crate) async fn open_token(
    tx: &mut DbTx,
    token_hash: &[u8],
    now: Timestamp,
) -> Result<Option<OpenToken>, DbError> {
    let row = sqlx::query_as::<_, (Uuid, bool, String, Option<Uuid>)>(OPEN_TOKEN)
        .bind(token_hash)
        .bind(now.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?;
    Ok(
        row.map(|(id, is_setup, grants_role, target_account_id)| OpenToken {
            id,
            is_setup,
            grants_role,
            target_account_id,
        }),
    )
}

/// Redeems invite `id` for `account` at `now`; `false` when it was redeemed meanwhile or has expired.
pub(crate) async fn redeem(
    tx: &mut DbTx,
    id: Uuid,
    account: Uuid,
    now: Timestamp,
) -> Result<bool, DbError> {
    Ok(sqlx::query(REDEEM)
        .bind(id)
        .bind(account)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?
        .rows_affected()
        == 1)
}

pub(crate) async fn insert(tx: &mut DbTx, invite: &NewInvite<'_>) -> Result<(), DbError> {
    let kind = match invite.kind {
        InviteKind::Invite => "invite",
        InviteKind::Setup => "setup",
    };
    sqlx::query(INSERT)
        .bind(invite.id)
        .bind(kind)
        .bind(invite.token_hash.as_bytes().as_slice())
        .bind(invite.grants_role)
        .bind(invite.target_account_id)
        .bind(invite.created_by)
        .bind(invite.note)
        .bind(invite.created_at.as_unix_micros())
        .bind(invite.expires_at.as_unix_micros())
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// Deletes the open setup token, if any, so its plaintext stops working.
pub(crate) async fn delete_open_setup(tx: &mut DbTx) -> Result<(), DbError> {
    sqlx::query(DELETE_OPEN_SETUP)
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// Deletes the open re-bind invites of `account`, so only the newest reset works.
pub(crate) async fn delete_open_rebind(tx: &mut DbTx, account: Uuid) -> Result<(), DbError> {
    sqlx::query(DELETE_OPEN_REBIND)
        .bind(account)
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// Member and operator admin invites, newest first.
pub(crate) async fn list(tx: &mut DbTx) -> Result<Vec<ListedInvite>, DbError> {
    let rows = sqlx::query_as::<_, ListedTuple>(LIST)
        .fetch_all(tx.conn())
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, note, created_at_micros, expires_at_micros, redeemed_at_micros)| ListedInvite {
                id,
                note,
                created_at_micros,
                expires_at_micros,
                redeemed_at_micros,
            },
        )
        .collect())
}

/// Deletes the unredeemed member or admin invite `id`; `false` when there is none (unknown, already revoked or
/// redeemed, a re-bind invite or the setup token).
pub(crate) async fn revoke(tx: &mut DbTx, id: Uuid) -> Result<bool, DbError> {
    Ok(sqlx::query(REVOKE)
        .bind(id)
        .execute(tx.conn())
        .await?
        .rows_affected()
        == 1)
}
