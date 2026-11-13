//! The statements on invite rows that account creation runs on its transaction: the read of an open invite
//! and its single conditional redemption.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use uuid::Uuid;

// Only a member or admin invite that creates an account: a setup token (`kind = 'setup'`) and a re-bind invite
// (`target_account_id` set) are redeemed by their own operation, never by account creation.
const OPEN_INVITE: &str = "SELECT id, grants_role FROM invites WHERE token_hash = $1 AND kind = 'invite' \
    AND target_account_id IS NULL AND redeemed_at IS NULL \
    AND expires_at > 'epoch'::timestamptz + $2::bigint * interval '1 microsecond'";

// The one statement that decides a redemption race: the second of two concurrent updates waits for the first to
// commit, re-reads the row and matches nothing.
const REDEEM: &str = "UPDATE invites \
    SET redeemed_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', redeemed_by = $2 \
    WHERE id = $1 AND redeemed_at IS NULL \
    AND expires_at > 'epoch'::timestamptz + $3::bigint * interval '1 microsecond'";

/// An invite that was open when read; the redemption decides whether it still is.
pub(crate) struct OpenInvite {
    pub id: Uuid,
    pub grants_role: String,
}

/// The account-creating invite whose token hashes to `token_hash`, when it is unredeemed and not expired at
/// `now`.
pub(crate) async fn open_invite(
    tx: &mut DbTx,
    token_hash: &[u8],
    now: Timestamp,
) -> Result<Option<OpenInvite>, DbError> {
    let row = sqlx::query_as::<_, (Uuid, String)>(OPEN_INVITE)
        .bind(token_hash)
        .bind(now.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(|(id, grants_role)| OpenInvite { id, grants_role }))
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
