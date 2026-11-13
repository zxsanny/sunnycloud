//! The statements that run before the caller's account is known (system scope) and the session insert of a
//! sign-in. The four system-scope doors of the device grant are here too, because a pending grant has no
//! account; their statements are in `device_grant_repo`. The credential probes of the authenticator are one statement each: a read on a unique `*_hash` index
//! (authenticate) or on the primary key (revalidate), joined to the account. Expiry, revocation and account
//! state are returned as columns and decided in code, never in the `WHERE` clause, so every dead credential
//! costs the same database work.

use std::sync::Arc;

use sc_clock::Timestamp;
use sc_db::{Db, DbError, RunError, Scope};
use sc_ids::TokenHash;
use thiserror::Error;
use uuid::Uuid;

use contracts::{AccountLifecycleHook, Decision, LifecycleError, PrincipalKind};
use sc_audit::{AuditError, AuditWriter};

use crate::account_repo::{self, ListedAccount, NewIdentity};
use crate::audit_events;
use crate::device_grant_repo::{
    self, Approver, DecideOutcome, NewDeviceToken, NewGrant, PendingGrant, PollStep,
};
use crate::invite_repo;
use crate::janitor_repo::{self, Target};
use crate::login_code_repo;
use crate::session_repo::{self, NewSession};

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

/// The columns of a consumed sign-in state that `complete` reads.
pub(crate) struct StateRow {
    pub provider: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub return_to: String,
    pub client_kind: String,
    /// The PKCE challenge of a mobile sign-in, copied to its login code.
    pub code_challenge: Option<String>,
    pub expires_at_micros: i64,
    /// The hash of the invite or setup token presented at `begin`, redeemed when the identity is unknown.
    pub invite_token_hash: Option<Vec<u8>>,
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
    RETURNING provider, nonce, pkce_verifier, return_to, client_kind, code_challenge, \
    (extract(epoch FROM expires_at) * 1000000)::bigint, invite_token_hash";

const IDENTITY_BY_PROVIDER_ID: &str = "SELECT i.id, i.account_id, a.state FROM identities i \
    JOIN accounts a ON a.id = i.account_id WHERE i.provider = $1 AND i.provider_user_id = $2";

type StateTuple = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    Option<Vec<u8>>,
);

/// The identity a provider user id belongs to, with the state of its account.
pub(crate) struct KnownIdentity {
    pub identity_id: Uuid,
    pub account_id: Uuid,
    pub account_state: String,
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
        code_challenge: row.5,
        expires_at_micros: row.6,
        invite_token_hash: row.7,
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

/// An account created from an invite: the rows identity writes and what the provider said about the user.
pub(crate) struct NewRegistration<'a> {
    pub invite_token_hash: &'a [u8],
    pub account_id: Uuid,
    pub display_name: &'a str,
    pub identity: NewIdentity<'a>,
    pub now: Timestamp,
}

/// Why the account-creation transaction rolled back.
#[derive(Debug, Error)]
pub(crate) enum CreateError {
    /// No open invite for the token, or a concurrent redemption won the race.
    #[error("the invite is not redeemable")]
    InviteNotRedeemable,
    #[error("an account lifecycle hook failed")]
    Hook(#[source] LifecycleError),
    /// The mandatory `account_created` event was not written; nothing was created.
    #[error("the account-created audit event was not written")]
    Audit(#[source] AuditError),
    /// The invite carries a role outside the `invites_grants_role` CHECK; a damaged database.
    #[error("unexpected invite role")]
    UnexpectedRole,
    #[error(transparent)]
    Db(#[from] DbError),
}

/// The audit code of an invite's `grants_role`.
fn role_code(role: &str) -> Option<&'static str> {
    match role {
        "user" => Some("user"),
        "admin" => Some("admin"),
        _ => None,
    }
}

/// Creates the account in one transaction: the `accounts` and `identities` rows, `initialize` of every hook in
/// registration order, the conditional redemption, then the mandatory `account_created` audit event as the last
/// statement. Any failure rolls all of it back, so the invite stays redeemable. System scope: the invite is
/// resolved before any account exists, and the feed hook reads the installation epoch from the other accounts'
/// rows.
pub(crate) async fn create_account(
    db: &Db,
    registration: &NewRegistration<'_>,
    hooks: &[Arc<dyn AccountLifecycleHook>],
    audit: &AuditWriter,
) -> Result<(), RunError<CreateError>> {
    let (account, now) = (registration.account_id, registration.now);
    db.run_system("account_create", async |tx| {
        let invite = invite_repo::open_invite(tx, registration.invite_token_hash, now)
            .await?
            .ok_or(CreateError::InviteNotRedeemable)?;
        account_repo::insert_account(
            tx,
            account,
            registration.display_name,
            &invite.grants_role,
            now,
        )
        .await?;
        account_repo::insert_identity(tx, &registration.identity).await?;
        for hook in hooks {
            hook.initialize(tx, account)
                .await
                .map_err(CreateError::Hook)?;
        }
        if !invite_repo::redeem(tx, invite.id, account, now).await? {
            return Err(CreateError::InviteNotRedeemable);
        }
        let role = role_code(&invite.grants_role).ok_or(CreateError::UnexpectedRole)?;
        let event =
            audit_events::account_created(account, registration.identity.id, invite.id, role);
        audit.record_in(tx, event).await.map_err(CreateError::Audit)
    })
    .await
}

/// Whether any account exists. Read before any caller is known: system scope.
pub(crate) async fn any_account(db: &Db) -> Result<bool, RunError<DbError>> {
    db.run_system("provider_list", async |tx| {
        account_repo::any_account(tx).await
    })
    .await
}

/// The new session and the identity's `last_login_at`, in one transaction of the account.
pub(crate) async fn start_session(
    db: &Db,
    identity: &KnownIdentity,
    session: NewSession<'_>,
) -> Result<(), RunError<DbError>> {
    db.run(Scope::Account(identity.account_id), async |tx| {
        session_repo::insert(tx, identity.account_id, &session).await?;
        account_repo::touch_identity(tx, identity.identity_id, session.created_at).await
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

/// One batch of the janitor: it spans every account, so it runs in system scope.
pub(crate) async fn sweep_batch(
    db: &Db,
    target: Target,
    cutoff: Timestamp,
    limit: i64,
) -> Result<u64, RunError<DbError>> {
    db.run_system("identity_janitor", async |tx| {
        janitor_repo::delete_batch(tx, target, cutoff, limit).await
    })
    .await
}

/// Consumes a mobile login code and mints its session in one transaction. The code is looked up by hash
/// before its account is known, so the transaction runs in system scope.
pub(crate) async fn exchange_login_code(
    db: &Db,
    code_hash: &TokenHash,
    attempt: &login_code_repo::Attempt<'_>,
) -> Result<login_code_repo::Exchange, RunError<DbError>> {
    db.run_system("mobile_code_exchange", async |tx| {
        login_code_repo::exchange(tx, code_hash, attempt).await
    })
    .await
}

/// The accounts page of the admin list. The last login is read from the identities of every account, so the
/// read runs in system scope.
pub(crate) async fn list_accounts(
    db: &Db,
    role: Option<&str>,
    state: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ListedAccount>, RunError<DbError>> {
    db.run_system("account_list", async |tx| {
        account_repo::list(tx, role, state, limit, offset).await
    })
    .await
}
