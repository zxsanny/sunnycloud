//! The statements that run before the caller's account is known (system scope) and the session insert of a
//! sign-in. The four system-scope doors of the device grant are here too, because a pending grant has no
//! account; their statements are in `device_grant_repo`. So are the doors of the invites and the setup token
//! (`invites` belongs to no account) and of the operator's admin reset, whose statements are in `invite_repo`,
//! `registration_repo` and `operator_repo`. The credential probes of the authenticator are one statement each: a read on a unique `*_hash` index
//! (authenticate) or on the primary key (revalidate), joined to the account. Expiry, revocation and account
//! state are returned as columns and decided in code, never in the `WHERE` clause, so every dead credential
//! costs the same database work.

use std::sync::Arc;

use sc_clock::Timestamp;
use sc_db::{Db, DbError, RunError, Scope};
use sc_ids::TokenHash;
use uuid::Uuid;

use contracts::{AccountLifecycleHook, Decision, PrincipalKind};
use sc_audit::AuditWriter;

use crate::account_repo::{self, ListedAccount};
use crate::device_grant_repo::{
    self, Approver, DecideOutcome, NewDeviceToken, NewGrant, PendingGrant, PollStep,
};
use crate::invite_repo::{self, ListedInvite, NewInvite};
use crate::janitor_repo::{self, Target};
use crate::login_code_repo;
use crate::operator_repo::{self, ResetError};
use crate::registration_repo::{self, CreateError, NewRegistration, Redeemed};
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

/// Redeems the token an unknown identity presented (`registration_repo`): an account created from an invite or
/// the setup token, or the identity bound to a re-bind invite's account. System scope: the token is resolved
/// before any account is known, and the feed hook reads the installation epoch from the other accounts' rows.
pub(crate) async fn redeem_invite(
    db: &Db,
    registration: &NewRegistration<'_>,
    hooks: Option<&[Arc<dyn AccountLifecycleHook>]>,
    audit: &AuditWriter,
) -> Result<Redeemed, RunError<CreateError>> {
    db.run_system("account_create", async |tx| {
        registration_repo::redeem(tx, registration, hooks, audit).await
    })
    .await
}

/// Stores a new token row. `invites` belongs to no account: system scope.
pub(crate) async fn insert_invite(
    db: &Db,
    invite: &NewInvite<'_>,
) -> Result<(), RunError<DbError>> {
    db.run_system("invite_create", async |tx| {
        invite_repo::insert(tx, invite).await
    })
    .await
}

/// Member and operator admin invites for the admin list; `invites` belongs to no account: system scope.
pub(crate) async fn list_invites(db: &Db) -> Result<Vec<ListedInvite>, RunError<DbError>> {
    db.run_system("invite_list", async |tx| invite_repo::list(tx).await)
        .await
}

/// Revokes an open member or admin invite; `false` when there is none. System scope, as `insert_invite`.
pub(crate) async fn revoke_invite(db: &Db, id: Uuid) -> Result<bool, RunError<DbError>> {
    db.run_system("invite_revoke", async |tx| {
        invite_repo::revoke(tx, id).await
    })
    .await
}

/// The setup token, issued only while no account exists: the check reads every account, system scope.
pub(crate) async fn replace_setup_token(
    db: &Db,
    setup: &NewInvite<'_>,
) -> Result<bool, RunError<DbError>> {
    db.run_system("setup_token", async |tx| {
        operator_repo::replace_setup_token(tx, setup).await
    })
    .await
}

/// The operator's admin reset (no caller account): it writes the account's credentials and an invite, system
/// scope.
pub(crate) async fn reset_admin(
    db: &Db,
    account: Uuid,
    rebind: &NewInvite<'_>,
    now: Timestamp,
    audit: &AuditWriter,
) -> Result<(), RunError<ResetError>> {
    db.run_system("admin_reset", async |tx| {
        operator_repo::reset_admin(tx, account, rebind, now, audit).await
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
