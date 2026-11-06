//! The account-scoped statements of the session lifecycle: rotation under the row lock, revocation, the list and
//! the `last_seen_at` write. Each function runs on the caller's transaction (`Db::run(Scope::Account(..))`), so
//! the service decides between the lock and the write without a second round trip. Expiry, revocation and
//! account state are returned as columns and decided in code.

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::TokenHash;
use uuid::Uuid;

use contracts::PrincipalKind;

/// Columns of a session row that the rotation decision reads, read under the row lock.
pub(crate) struct LockedSession {
    pub refresh_hash: Vec<u8>,
    pub prev_refresh_hash: Option<Vec<u8>>,
    pub rotated_at_micros: Option<i64>,
    pub refresh_expires_at_micros: i64,
    pub revoked: bool,
    pub account_state: String,
}

type LockedTuple = (Vec<u8>, Option<Vec<u8>>, Option<i64>, i64, bool, String);

/// The successor credentials written by a rotation.
pub(crate) struct Rotation {
    pub token_hash: TokenHash,
    pub access_expires_at: Timestamp,
    pub refresh_hash: TokenHash,
    pub refresh_expires_at: Timestamp,
    pub rotated_at: Timestamp,
}

/// A listed session; revocation and expiry are decided by the caller.
pub(crate) struct ListedSession {
    pub id: Uuid,
    pub label: String,
    pub created_at: Timestamp,
    pub last_seen_at: Timestamp,
    pub revoked: bool,
    pub refresh_expires_at_micros: i64,
}

type ListedTuple = (Uuid, String, i64, i64, bool, i64);

const LOCK_SESSION: &str = "SELECT s.refresh_hash, s.prev_refresh_hash, \
    (extract(epoch FROM s.rotated_at) * 1000000)::bigint, \
    (extract(epoch FROM s.refresh_expires_at) * 1000000)::bigint, s.revoked_at IS NOT NULL, a.state \
    FROM sessions s JOIN accounts a ON a.id = s.account_id WHERE s.id = $1 FOR UPDATE OF s";

const ROTATE: &str = "UPDATE sessions SET prev_refresh_hash = refresh_hash, refresh_hash = $2, token_hash = $3, \
    access_expires_at = 'epoch'::timestamptz + $4::bigint * interval '1 microsecond', \
    refresh_expires_at = 'epoch'::timestamptz + $5::bigint * interval '1 microsecond', \
    rotated_at = 'epoch'::timestamptz + $6::bigint * interval '1 microsecond' WHERE id = $1";

const REVOKE_ONE: &str = "UPDATE sessions \
    SET revoked_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', revoke_reason = $4 \
    WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL";

const REVOKE_ALL: &str = "UPDATE sessions \
    SET revoked_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', revoke_reason = $3 \
    WHERE account_id = $1 AND revoked_at IS NULL";

const LIST: &str = "SELECT id, label, \
    (extract(epoch FROM created_at) * 1000000)::bigint, \
    (extract(epoch FROM COALESCE(last_seen_at, created_at)) * 1000000)::bigint, \
    revoked_at IS NOT NULL, (extract(epoch FROM refresh_expires_at) * 1000000)::bigint \
    FROM sessions WHERE account_id = $1 ORDER BY created_at DESC, id";

// A touch inside the interval matches no row, so it writes nothing.
const TOUCH_SESSION: &str = "UPDATE sessions \
    SET last_seen_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
    WHERE id = $1 AND revoked_at IS NULL \
    AND (last_seen_at IS NULL OR last_seen_at <= 'epoch'::timestamptz + $3::bigint * interval '1 microsecond')";

const TOUCH_DEVICE: &str = "UPDATE device_tokens \
    SET last_seen_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
    WHERE id = $1 AND revoked_at IS NULL \
    AND (last_seen_at IS NULL OR last_seen_at <= 'epoch'::timestamptz + $3::bigint * interval '1 microsecond')";

/// Takes the row lock of session `id` until the transaction ends; concurrent rotations of the session queue here.
pub(crate) async fn lock(tx: &mut DbTx, id: Uuid) -> Result<Option<LockedSession>, DbError> {
    let row = sqlx::query_as::<_, LockedTuple>(LOCK_SESSION)
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(
        |(
            refresh_hash,
            prev_refresh_hash,
            rotated_at_micros,
            refresh_expires_at_micros,
            revoked,
            account_state,
        )| LockedSession {
            refresh_hash,
            prev_refresh_hash,
            rotated_at_micros,
            refresh_expires_at_micros,
            revoked,
            account_state,
        },
    ))
}

/// Moves the current refresh hash to `prev_refresh_hash` and installs the successor pair.
pub(crate) async fn rotate(tx: &mut DbTx, id: Uuid, rotation: &Rotation) -> Result<(), DbError> {
    sqlx::query(ROTATE)
        .bind(id)
        .bind(rotation.refresh_hash.as_bytes().as_slice())
        .bind(rotation.token_hash.as_bytes().as_slice())
        .bind(rotation.access_expires_at.as_unix_micros())
        .bind(rotation.refresh_expires_at.as_unix_micros())
        .bind(rotation.rotated_at.as_unix_micros())
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// Revokes one live session of `account`; a foreign, unknown or already revoked id changes no row.
pub(crate) async fn revoke(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    at: Timestamp,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(REVOKE_ONE)
        .bind(id)
        .bind(account)
        .bind(at.as_unix_micros())
        .bind(reason)
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

pub(crate) async fn revoke_all(
    tx: &mut DbTx,
    account: Uuid,
    at: Timestamp,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(REVOKE_ALL)
        .bind(account)
        .bind(at.as_unix_micros())
        .bind(reason)
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// Every session row of `account`, newest first; a session never seen again reports its creation as last seen.
pub(crate) async fn list(tx: &mut DbTx, account: Uuid) -> Result<Vec<ListedSession>, DbError> {
    let rows = sqlx::query_as::<_, ListedTuple>(LIST)
        .bind(account)
        .fetch_all(tx.conn())
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, label, created, last_seen, revoked, refresh_expires_at_micros)| ListedSession {
                id,
                label,
                created_at: Timestamp::from_unix_micros(created),
                last_seen_at: Timestamp::from_unix_micros(last_seen),
                revoked,
                refresh_expires_at_micros,
            },
        )
        .collect())
}

/// Writes `last_seen_at = at` unless it was written at or after `not_after` (that is, within the interval).
pub(crate) async fn touch(
    tx: &mut DbTx,
    kind: PrincipalKind,
    id: Uuid,
    at: Timestamp,
    not_after: Timestamp,
) -> Result<(), DbError> {
    let sql = match kind {
        PrincipalKind::Session => TOUCH_SESSION,
        PrincipalKind::Device => TOUCH_DEVICE,
    };
    sqlx::query(sql)
        .bind(id)
        .bind(at.as_unix_micros())
        .bind(not_after.as_unix_micros())
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}
