//! The statements that run before the caller's account is known (system scope) and the session insert of a
//! sign-in. The four system-scope doors of the device grant are here too, because a pending grant has no
//! account; their statements are in `device_grant_repo`. The credential probes of the authenticator are one statement each: a read on a unique `*_hash` index
//! (authenticate) or on the primary key (revalidate), joined to the account. Expiry, revocation and account
//! state are returned as columns and decided in code, never in the `WHERE` clause, so every dead credential
//! costs the same database work.

use sc_clock::Timestamp;
use sc_db::{Db, DbError, RunError, Scope};
use sc_ids::TokenHash;
use uuid::Uuid;

use contracts::{Decision, PrincipalKind};

use crate::device_grant_repo::{
    self, Approver, DecideOutcome, NewDeviceToken, NewGrant, PendingGrant, PollStep,
};

/// Columns of a probe, undecided.
pub(crate) struct CredentialRow {
    pub id: Uuid,
    pub account_id: Uuid,
    pub role: String,
    pub account_state: String,
    pub label: String,
    /// Access-token expiry in microseconds since the epoch; `None` for a device token, which never expires.
    pub expires_at_micros: Option<i64>,
    pub revoked: bool,
}

type RowTuple = (Uuid, Uuid, String, String, String, Option<i64>, bool);

impl From<RowTuple> for CredentialRow {
    fn from(row: RowTuple) -> Self {
        let (id, account_id, role, account_state, label, expires_at_micros, revoked) = row;
        CredentialRow {
            id,
            account_id,
            role,
            account_state,
            label,
            expires_at_micros,
            revoked,
        }
    }
}

const SESSION_BY_HASH: &str = "SELECT s.id, s.account_id, a.role, a.state, s.label, \
    (extract(epoch FROM s.access_expires_at) * 1000000)::bigint, s.revoked_at IS NOT NULL \
    FROM sessions s JOIN accounts a ON a.id = s.account_id WHERE s.token_hash = $1";

const DEVICE_BY_HASH: &str = "SELECT d.id, d.account_id, a.role, a.state, d.device_name, \
    NULL::bigint, d.revoked_at IS NOT NULL \
    FROM device_tokens d JOIN accounts a ON a.id = d.account_id WHERE d.token_hash = $1";

const SESSION_BY_ID: &str = "SELECT s.id, s.account_id, a.role, a.state, s.label, \
    (extract(epoch FROM s.access_expires_at) * 1000000)::bigint, s.revoked_at IS NOT NULL \
    FROM sessions s JOIN accounts a ON a.id = s.account_id WHERE s.id = $1";

const DEVICE_BY_ID: &str = "SELECT d.id, d.account_id, a.role, a.state, d.device_name, \
    NULL::bigint, d.revoked_at IS NOT NULL \
    FROM device_tokens d JOIN accounts a ON a.id = d.account_id WHERE d.id = $1";

/// The credential whose token hashes to `hash`. The account is not known yet, so the read runs in system
/// scope (the tables carry row-level security by account).
pub(crate) async fn by_hash(
    db: &Db,
    kind: PrincipalKind,
    hash: &TokenHash,
) -> Result<Option<CredentialRow>, RunError<DbError>> {
    let sql = match kind {
        PrincipalKind::Session => SESSION_BY_HASH,
        PrincipalKind::Device => DEVICE_BY_HASH,
    };
    let row = db
        .run_system("authenticate", async |tx| {
            sqlx::query_as::<_, RowTuple>(sql)
                .bind(hash.as_bytes().as_slice())
                .fetch_optional(tx.conn())
                .await
                .map_err(DbError::from)
        })
        .await?;
    Ok(row.map(CredentialRow::from))
}

/// The session a presented refresh token belongs to, current or previous generation.
pub(crate) struct RefreshOwner {
    pub session_id: Uuid,
    pub account_id: Uuid,
}

const SESSION_BY_REFRESH: &str = "SELECT id, account_id FROM sessions WHERE refresh_hash = $1";

const SESSION_BY_PREV_REFRESH: &str =
    "SELECT id, account_id FROM sessions WHERE prev_refresh_hash = $1";

/// The session whose current refresh hash, else whose previous one, is `hash`. The account is not known yet,
/// so the read runs in system scope. The current hash is indexed and answers a normal refresh alone; the
/// previous hash is probed only when the first read finds nothing. Nothing is locked or decided here.
pub(crate) async fn session_by_refresh_hash(
    db: &Db,
    hash: &TokenHash,
) -> Result<Option<RefreshOwner>, RunError<DbError>> {
    let row = db
        .run_system("refresh_lookup", async |tx| {
            let current = sqlx::query_as::<_, (Uuid, Uuid)>(SESSION_BY_REFRESH)
                .bind(hash.as_bytes().as_slice())
                .fetch_optional(tx.conn())
                .await
                .map_err(DbError::from)?;
            if current.is_some() {
                return Ok(current);
            }
            sqlx::query_as::<_, (Uuid, Uuid)>(SESSION_BY_PREV_REFRESH)
                .bind(hash.as_bytes().as_slice())
                .fetch_optional(tx.conn())
                .await
                .map_err(DbError::from)
        })
        .await?;
    Ok(row.map(|(session_id, account_id)| RefreshOwner {
        session_id,
        account_id,
    }))
}

/// The credential `id` of `account`.
pub(crate) async fn by_id(
    db: &Db,
    kind: PrincipalKind,
    account: Uuid,
    id: Uuid,
) -> Result<Option<CredentialRow>, RunError<DbError>> {
    let sql = match kind {
        PrincipalKind::Session => SESSION_BY_ID,
        PrincipalKind::Device => DEVICE_BY_ID,
    };
    let row = db
        .run(Scope::Account(account), async |tx| {
            sqlx::query_as::<_, RowTuple>(sql)
                .bind(id)
                .fetch_optional(tx.conn())
                .await
                .map_err(DbError::from)
        })
        .await?;
    Ok(row.map(CredentialRow::from))
}

/// The columns of a consumed sign-in state that `complete` reads. The invite hash and the mobile code challenge
/// are consumed with the row and read by the tasks that redeem them.
pub(crate) struct StateRow {
    pub provider: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub return_to: String,
    pub client_kind: String,
    pub expires_at_micros: i64,
}

pub(crate) struct NewState<'a> {
    pub state_hash: &'a TokenHash,
    pub provider: &'a str,
    pub nonce: &'a str,
    pub pkce_verifier: &'a str,
    pub return_to: &'a str,
    pub client_kind: &'a str,
    pub invite_token_hash: Option<TokenHash>,
    pub code_challenge: Option<&'a str>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

const INSERT_STATE: &str = "INSERT INTO oauth_states (state_hash, provider, nonce, pkce_verifier, return_to, \
    client_kind, invite_token_hash, code_challenge, created_at, expires_at) \
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, \
    'epoch'::timestamptz + $9::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $10::bigint * interval '1 microsecond')";

// Expiry is returned and decided in code: an expired state is consumed like a live one and fails the same way.
const CONSUME_STATE: &str = "DELETE FROM oauth_states WHERE state_hash = $1 \
    RETURNING provider, nonce, pkce_verifier, return_to, client_kind, \
    (extract(epoch FROM expires_at) * 1000000)::bigint";

const IDENTITY_BY_PROVIDER_ID: &str = "SELECT i.id, i.account_id, a.state FROM identities i \
    JOIN accounts a ON a.id = i.account_id WHERE i.provider = $1 AND i.provider_user_id = $2";

const INSERT_SESSION: &str = "INSERT INTO sessions (id, account_id, client_kind, label, token_hash, \
    access_expires_at, refresh_hash, refresh_expires_at, created_at) \
    VALUES ($1, $2, $3, $4, $5, \
    'epoch'::timestamptz + $6::bigint * interval '1 microsecond', $7, \
    'epoch'::timestamptz + $8::bigint * interval '1 microsecond', \
    'epoch'::timestamptz + $9::bigint * interval '1 microsecond')";

const TOUCH_IDENTITY: &str = "UPDATE identities \
    SET last_login_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' WHERE id = $1";

type StateTuple = (String, String, String, String, String, i64);

/// The identity a provider user id belongs to, with the state of its account.
pub(crate) struct KnownIdentity {
    pub identity_id: Uuid,
    pub account_id: Uuid,
    pub account_state: String,
}

pub(crate) struct NewSession<'a> {
    pub id: Uuid,
    pub client_kind: &'a str,
    pub label: &'a str,
    pub token_hash: TokenHash,
    pub access_expires_at: Timestamp,
    pub refresh_hash: TokenHash,
    pub refresh_expires_at: Timestamp,
    pub created_at: Timestamp,
}

/// States are read before any account is known, and `oauth_states` carries no row policy: system scope.
pub(crate) async fn insert_state(db: &Db, state: NewState<'_>) -> Result<(), RunError<DbError>> {
    db.run_system("sign_in_begin", async |tx| {
        sqlx::query(INSERT_STATE)
            .bind(state.state_hash.as_bytes().as_slice())
            .bind(state.provider)
            .bind(state.nonce)
            .bind(state.pkce_verifier)
            .bind(state.return_to)
            .bind(state.client_kind)
            .bind(
                state
                    .invite_token_hash
                    .map(|hash| hash.into_bytes().to_vec()),
            )
            .bind(state.code_challenge)
            .bind(state.created_at.as_unix_micros())
            .bind(state.expires_at.as_unix_micros())
            .execute(tx.conn())
            .await
            .map(drop)
            .map_err(DbError::from)
    })
    .await
}

/// Consumes the state: `DELETE ... RETURNING`, so a replay, a foreign binding or a second callback finds
/// nothing.
pub(crate) async fn consume_state(
    db: &Db,
    state_hash: &TokenHash,
) -> Result<Option<StateRow>, RunError<DbError>> {
    let row = db
        .run_system("sign_in_callback", async |tx| {
            sqlx::query_as::<_, StateTuple>(CONSUME_STATE)
                .bind(state_hash.as_bytes().as_slice())
                .fetch_optional(tx.conn())
                .await
                .map_err(DbError::from)
        })
        .await?;
    Ok(row.map(|row| StateRow {
        provider: row.0,
        nonce: row.1,
        pkce_verifier: row.2,
        return_to: row.3,
        client_kind: row.4,
        expires_at_micros: row.5,
    }))
}

/// Looks the identity up by `(provider, provider_user_id)`; never by email. The account is not known yet,
/// so the read runs in system scope.
pub(crate) async fn identity_by_provider_id(
    db: &Db,
    provider: &str,
    provider_user_id: &str,
) -> Result<Option<KnownIdentity>, RunError<DbError>> {
    let row = db
        .run_system("sign_in_identity", async |tx| {
            sqlx::query_as::<_, (Uuid, Uuid, String)>(IDENTITY_BY_PROVIDER_ID)
                .bind(provider)
                .bind(provider_user_id)
                .fetch_optional(tx.conn())
                .await
                .map_err(DbError::from)
        })
        .await?;
    Ok(
        row.map(|(identity_id, account_id, account_state)| KnownIdentity {
            identity_id,
            account_id,
            account_state,
        }),
    )
}

/// The new session and the identity's `last_login_at`, in one transaction of the account.
pub(crate) async fn start_session(
    db: &Db,
    identity: &KnownIdentity,
    session: NewSession<'_>,
) -> Result<(), RunError<DbError>> {
    db.run(Scope::Account(identity.account_id), async |tx| {
        sqlx::query(INSERT_SESSION)
            .bind(session.id)
            .bind(identity.account_id)
            .bind(session.client_kind)
            .bind(session.label)
            .bind(session.token_hash.as_bytes().as_slice())
            .bind(session.access_expires_at.as_unix_micros())
            .bind(session.refresh_hash.as_bytes().as_slice())
            .bind(session.refresh_expires_at.as_unix_micros())
            .bind(session.created_at.as_unix_micros())
            .execute(tx.conn())
            .await
            .map_err(DbError::from)?;
        sqlx::query(TOUCH_IDENTITY)
            .bind(identity.identity_id)
            .bind(session.created_at.as_unix_micros())
            .execute(tx.conn())
            .await
            .map(drop)
            .map_err(DbError::from)
    })
    .await
}

/// Stores a new pending grant; a duplicate user code is a unique violation for the caller to retry. A pending
/// grant has no account, so the insert runs in system scope.
pub(crate) async fn insert_grant(db: &Db, grant: &NewGrant<'_>) -> Result<(), RunError<DbError>> {
    db.run_system("device_request", async |tx| {
        device_grant_repo::insert(tx, grant).await
    })
    .await
}

/// One poll by device-code hash, which is all the poller presents: the grant's account is unknown (NULL while
/// pending), so the transaction runs in system scope.
pub(crate) async fn poll_grant(
    db: &Db,
    code_hash: &TokenHash,
    token: &NewDeviceToken<'_>,
    now: Timestamp,
    polled_before: Timestamp,
) -> Result<PollStep, RunError<DbError>> {
    db.run_system("device_poll", async |tx| {
        device_grant_repo::poll(tx, code_hash, token, now, polled_before).await
    })
    .await
}

/// The pending grant of a user code: the approver is not its owner (a pending grant has no account), so the
/// read runs in system scope.
pub(crate) async fn pending_grant_by_user_code(
    db: &Db,
    user_code: &str,
    now: Timestamp,
) -> Result<Option<PendingGrant>, RunError<DbError>> {
    db.run_system("device_lookup", async |tx| {
        device_grant_repo::pending_by_user_code(tx, user_code, now).await
    })
    .await
}

/// The decision on a grant that is still pending and so belongs to no account: system scope.
pub(crate) async fn decide_grant(
    db: &Db,
    id: Uuid,
    decision: Decision,
    approver: &Approver,
    now: Timestamp,
) -> Result<DecideOutcome, RunError<DbError>> {
    db.run_system("device_decide", async |tx| {
        device_grant_repo::decide(tx, id, decision, approver, now).await
    })
    .await
}
