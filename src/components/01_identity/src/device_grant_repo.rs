//! The statements of the device grant and the device list. A pending grant has no account, so row-level
//! security hides it from every account scope: the grant statements run on the system-scope transactions of
//! `auth_repo`. Expiry is decided in the `WHERE` of each conditional update, against the time bound by the
//! caller (the clock), never against the database clock. The statements that list and revoke devices run in
//! the account scope.

use contracts::Decision;
use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use sc_ids::TokenHash;
use uuid::Uuid;

pub(crate) struct NewGrant<'a> {
    pub id: Uuid,
    pub device_code_hash: &'a TokenHash,
    pub user_code: &'a str,
    pub requester_ip: String,
    pub requester_location: Option<&'a str>,
    pub device_name: &'a str,
    pub platform: &'a str,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// The device token a successful poll inserts; its secret is hashed by the caller.
pub(crate) struct NewDeviceToken<'a> {
    pub id: Uuid,
    pub token_hash: &'a TokenHash,
    pub created_at: Timestamp,
}

/// What a poll did.
pub(crate) enum PollStep {
    /// The grant moved `approved` to `consumed` and the token row was inserted.
    Minted,
    /// The grant is pending and was not polled too fast: `last_polled_at` is now.
    Touched,
    /// Nothing changed. `None`: no grant has this code hash.
    Refused(Option<GrantState>),
}

/// Status and expiry of a grant, undecided.
pub(crate) struct GrantState {
    pub status: String,
    pub expires_at_micros: i64,
}

/// A pending, unexpired grant as the approver sees it. The address is `host(inet)` text.
pub(crate) struct PendingGrant {
    pub id: Uuid,
    pub expires_at_micros: i64,
    pub device_name: String,
    pub requester_ip: String,
    pub requester_location: Option<String>,
}

pub(crate) struct Approver {
    pub account_id: Uuid,
    pub session_id: Uuid,
}

/// What `decide` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecideOutcome {
    Applied,
    /// Still pending past its expiry: closed now, so a second decision finds it decided.
    Expired,
    AlreadyDecided,
    Unknown,
}

pub(crate) struct ListedDevice {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub created_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
}

type DeviceTuple = (Uuid, String, String, i64, Option<i64>);
type PendingTuple = (Uuid, i64, String, String, Option<String>);

const INSERT_GRANT: &str = "INSERT INTO device_grants (id, device_code_hash, user_code, requester_ip, \
    requester_location, device_name, platform, status, created_at, expires_at) \
    VALUES ($1, $2, $3, $4::text::inet, $5, $6, $7, 'pending', \
    'epoch'::timestamptz + $8::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $9::bigint * interval '1 microsecond')";

// The conditional update is the mint: of any number of concurrent polls, the row lock lets one see
// `approved`; the others re-check the `WHERE` after it commits and find `consumed`.
const CONSUME_GRANT: &str = "UPDATE device_grants SET status = 'consumed' \
    WHERE device_code_hash = $1 AND status = 'approved' \
    AND expires_at > 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
    RETURNING account_id, device_name, platform";

const INSERT_TOKEN: &str = "INSERT INTO device_tokens (id, account_id, token_hash, device_name, platform, created_at) \
    VALUES ($1, $2, $3, $4, $5, 'epoch'::timestamptz + $6::bigint * interval '1 microsecond')";

const TOUCH_GRANT: &str = "UPDATE device_grants \
    SET last_polled_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
    WHERE device_code_hash = $1 AND status = 'pending' \
    AND expires_at > 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' \
    AND (last_polled_at IS NULL \
    OR last_polled_at <= 'epoch'::timestamptz + $3::bigint * interval '1 microsecond')";

const GRANT_STATE: &str = "SELECT status, (extract(epoch FROM expires_at) * 1000000)::bigint \
    FROM device_grants WHERE device_code_hash = $1";

const PENDING_BY_USER_CODE: &str = "SELECT id, (extract(epoch FROM expires_at) * 1000000)::bigint, \
    device_name, host(requester_ip), requester_location FROM device_grants \
    WHERE user_code = $1 AND status = 'pending' \
    AND expires_at > 'epoch'::timestamptz + $2::bigint * interval '1 microsecond'";

const DECIDE_GRANT: &str = "UPDATE device_grants SET status = $2, account_id = $3, \
    approved_by_session_id = $4, \
    approved_at = 'epoch'::timestamptz + $5::bigint * interval '1 microsecond' \
    WHERE id = $1 AND status = 'pending' \
    AND expires_at > 'epoch'::timestamptz + $5::bigint * interval '1 microsecond'";

// There is no `expired` status: a code that ran out undecided is closed as `denied`, and polls report
// expiry before the status.
const CLOSE_EXPIRED_GRANT: &str = "UPDATE device_grants SET status = 'denied' \
    WHERE id = $1 AND status = 'pending' \
    AND expires_at <= 'epoch'::timestamptz + $2::bigint * interval '1 microsecond'";

const GRANT_EXISTS: &str = "SELECT 1 FROM device_grants WHERE id = $1";

const LIST_DEVICES: &str = "SELECT id, device_name, platform, \
    (extract(epoch FROM created_at) * 1000000)::bigint, \
    (extract(epoch FROM last_seen_at) * 1000000)::bigint \
    FROM device_tokens WHERE account_id = $1 AND revoked_at IS NULL ORDER BY created_at, id";

const REVOKE_DEVICE: &str = "UPDATE device_tokens \
    SET revoked_at = 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', revoke_reason = 'user' \
    WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL";

pub(crate) async fn insert(tx: &mut DbTx, grant: &NewGrant<'_>) -> Result<(), DbError> {
    sqlx::query(INSERT_GRANT)
        .bind(grant.id)
        .bind(grant.device_code_hash.as_bytes().as_slice())
        .bind(grant.user_code)
        .bind(&grant.requester_ip)
        .bind(grant.requester_location)
        .bind(grant.device_name)
        .bind(grant.platform)
        .bind(grant.created_at.as_unix_micros())
        .bind(grant.expires_at.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(())
}

/// One poll, in the caller's transaction. The mint is tried first, then the pending touch; only when both
/// change nothing is the grant read, to tell the caller why. `polled_before` is `now` minus the poll
/// interval: a pending grant polled after that is answered, one polled later than that is too fast.
pub(crate) async fn poll(
    tx: &mut DbTx,
    code_hash: &TokenHash,
    token: &NewDeviceToken<'_>,
    now: Timestamp,
    polled_before: Timestamp,
) -> Result<PollStep, DbError> {
    // An approved grant always has its account; a NULL one fails the decode and rolls the consume back.
    let minted = sqlx::query_as::<_, (Uuid, String, String)>(CONSUME_GRANT)
        .bind(code_hash.as_bytes().as_slice())
        .bind(now.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?;
    if let Some((account, device_name, platform)) = minted {
        sqlx::query(INSERT_TOKEN)
            .bind(token.id)
            .bind(account)
            .bind(token.token_hash.as_bytes().as_slice())
            .bind(device_name)
            .bind(platform)
            .bind(token.created_at.as_unix_micros())
            .execute(tx.conn())
            .await?;
        return Ok(PollStep::Minted);
    }
    let touched = sqlx::query(TOUCH_GRANT)
        .bind(code_hash.as_bytes().as_slice())
        .bind(now.as_unix_micros())
        .bind(polled_before.as_unix_micros())
        .execute(tx.conn())
        .await?;
    if touched.rows_affected() == 1 {
        return Ok(PollStep::Touched);
    }
    let state = sqlx::query_as::<_, (String, i64)>(GRANT_STATE)
        .bind(code_hash.as_bytes().as_slice())
        .fetch_optional(tx.conn())
        .await?;
    Ok(PollStep::Refused(state.map(
        |(status, expires_at_micros)| GrantState {
            status,
            expires_at_micros,
        },
    )))
}

pub(crate) async fn pending_by_user_code(
    tx: &mut DbTx,
    user_code: &str,
    now: Timestamp,
) -> Result<Option<PendingGrant>, DbError> {
    let row = sqlx::query_as::<_, PendingTuple>(PENDING_BY_USER_CODE)
        .bind(user_code)
        .bind(now.as_unix_micros())
        .fetch_optional(tx.conn())
        .await?;
    Ok(row.map(
        |(id, expires_at_micros, device_name, requester_ip, requester_location)| PendingGrant {
            id,
            expires_at_micros,
            device_name,
            requester_ip,
            requester_location,
        },
    ))
}

/// Approves or denies the pending grant `id`. An approval records the approving account and session; a denial
/// records only the status.
pub(crate) async fn decide(
    tx: &mut DbTx,
    id: Uuid,
    decision: Decision,
    approver: &Approver,
    now: Timestamp,
) -> Result<DecideOutcome, DbError> {
    let (status, account, session) = match decision {
        Decision::Approve => (
            "approved",
            Some(approver.account_id),
            Some(approver.session_id),
        ),
        Decision::Deny => ("denied", None, None),
    };
    let applied = sqlx::query(DECIDE_GRANT)
        .bind(id)
        .bind(status)
        .bind(account)
        .bind(session)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    if applied.rows_affected() == 1 {
        return Ok(DecideOutcome::Applied);
    }
    let closed = sqlx::query(CLOSE_EXPIRED_GRANT)
        .bind(id)
        .bind(now.as_unix_micros())
        .execute(tx.conn())
        .await?;
    if closed.rows_affected() == 1 {
        return Ok(DecideOutcome::Expired);
    }
    let exists = sqlx::query_scalar::<_, i32>(GRANT_EXISTS)
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
    Ok(if exists.is_some() {
        DecideOutcome::AlreadyDecided
    } else {
        DecideOutcome::Unknown
    })
}

/// The live device tokens of `account`, oldest first.
pub(crate) async fn list_devices(
    tx: &mut DbTx,
    account: Uuid,
) -> Result<Vec<ListedDevice>, DbError> {
    let rows = sqlx::query_as::<_, DeviceTuple>(LIST_DEVICES)
        .bind(account)
        .fetch_all(tx.conn())
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, platform, created, last_seen)| ListedDevice {
            id,
            name,
            platform,
            created_at: Timestamp::from_unix_micros(created),
            last_seen_at: last_seen.map(Timestamp::from_unix_micros),
        })
        .collect())
}

/// Revokes one live device token of `account`; returns whether a row changed. A foreign, unknown or already
/// revoked id changes none.
pub(crate) async fn revoke_device(
    tx: &mut DbTx,
    account: Uuid,
    id: Uuid,
    at: Timestamp,
) -> Result<bool, DbError> {
    let revoked = sqlx::query(REVOKE_DEVICE)
        .bind(id)
        .bind(account)
        .bind(at.as_unix_micros())
        .execute(tx.conn())
        .await?;
    Ok(revoked.rows_affected() == 1)
}
