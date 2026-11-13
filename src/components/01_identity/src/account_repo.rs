//! The statements on account rows and on the rows that belong to an account. Each runs on the caller's
//! transaction; the ones that change an account first take its row lock (`lock`).

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};
use uuid::Uuid;

const INSERT_ACCOUNT: &str = "INSERT INTO accounts (id, state, created_at, display_name, role) \
    VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', $3, $4)";

const INSERT_IDENTITY: &str = "INSERT INTO identities \
    (id, account_id, provider, provider_user_id, email, email_verified, created_at) \
    VALUES ($1, $2, $3, $4, $5, $6, 'epoch'::timestamptz + $7::bigint * interval '1 microsecond')";

/// The unique key an identity insert hits when the same provider identity was registered concurrently.
pub(crate) const IDENTITY_KEY: &str = "identities_provider_provider_user_id_key";

const TOUCH_IDENTITY: &str = "UPDATE identities \
    SET last_login_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' WHERE id = $1";

const LOCK_STATE: &str = "SELECT state FROM accounts WHERE id = $1 FOR UPDATE";

const SET_ROLE: &str = "UPDATE accounts SET role = $2 WHERE id = $1";

const SET_STATE: &str = "UPDATE accounts SET state = $2 WHERE id = $1";

// A repeated call keeps the time of the first request.
const MARK_DELETING: &str = "UPDATE accounts SET state = 'deleting', deletion_requested_at = \
    COALESCE(deletion_requested_at, 'epoch'::timestamptz + $2::bigint * interval '1 microsecond') WHERE id = $1";

// Ordered by id so that two callers locking the set take the rows in the same order.
const ACTIVE_ADMINS: &str =
    "SELECT id FROM accounts WHERE role = 'admin' AND state = 'active' ORDER BY id FOR UPDATE";

const LIST: &str = "SELECT a.id, a.display_name, a.role, a.state, \
    (SELECT (extract(epoch FROM max(i.last_login_at)) * 1000000)::bigint FROM identities i \
     WHERE i.account_id = a.id) \
    FROM accounts a WHERE ($1::text IS NULL OR a.role = $1) AND ($2::text IS NULL OR a.state = $2) \
    ORDER BY a.created_at, a.id LIMIT $3 OFFSET $4";

// Children before the account row: every reference to it is ON DELETE RESTRICT.
const DELETE_OWNED: [&str; 5] = [
    "DELETE FROM login_codes WHERE account_id = $1",
    "DELETE FROM device_grants WHERE account_id = $1",
    "DELETE FROM device_tokens WHERE account_id = $1",
    "DELETE FROM sessions WHERE account_id = $1",
    "DELETE FROM identities WHERE account_id = $1",
];

const DELETE_ACCOUNT: &str = "DELETE FROM accounts WHERE id = $1";

/// A listed account, undecided: role and state are the stored text.
pub(crate) struct ListedAccount {
    pub id: Uuid,
    pub display_name: String,
    pub role: String,
    pub state: String,
    pub last_login_micros: Option<i64>,
}

type ListedTuple = (Uuid, String, String, String, Option<i64>);

// The shared lock makes a concurrent `begin_deletion` (which takes the row for update) wait until this
// transaction ends, and the state read under it cannot change before then.
const STATE_SHARED: &str = "SELECT state FROM accounts WHERE id = $1 FOR SHARE";

/// An active account with the role an invite grants (`user` or `admin`).
pub(crate) async fn insert_account(
    tx: &mut DbTx,
    id: Uuid,
    display_name: &str,
    role: &str,
    at: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(INSERT_ACCOUNT)
        .bind(id)
        .bind(at.as_unix_micros())
        .bind(display_name)
        .bind(role)
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// The identity row of a new account. `email` is stored only as read under the email rule (verified, allowlist
/// configured); it is never a lookup key.
pub(crate) struct NewIdentity<'a> {
    pub id: Uuid,
    pub account_id: Uuid,
    pub provider: &'a str,
    pub provider_user_id: &'a str,
    pub email: Option<&'a str>,
    pub created_at: Timestamp,
}

pub(crate) async fn insert_identity(
    tx: &mut DbTx,
    identity: &NewIdentity<'_>,
) -> Result<(), DbError> {
    sqlx::query(INSERT_IDENTITY)
        .bind(identity.id)
        .bind(identity.account_id)
        .bind(identity.provider)
        .bind(identity.provider_user_id)
        .bind(identity.email)
        .bind(identity.email.is_some())
        .bind(identity.created_at.as_unix_micros())
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

pub(crate) async fn touch_identity(
    tx: &mut DbTx,
    identity: Uuid,
    at: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(TOUCH_IDENTITY)
        .bind(identity)
        .bind(at.as_unix_micros())
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

/// The state of `account` read under a shared row lock held until the transaction ends; `None` when the
/// account does not exist.
pub(crate) async fn state_shared(tx: &mut DbTx, account: Uuid) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar::<_, String>(STATE_SHARED)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?)
}

/// Takes the row lock of `account` until the transaction ends and returns its state; `None` when it does not
/// exist. Concurrent guards (shared lock) finish first, later ones wait.
pub(crate) async fn lock(tx: &mut DbTx, account: Uuid) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar::<_, String>(LOCK_STATE)
        .bind(account)
        .fetch_optional(tx.conn())
        .await?)
}

/// Whether the account exists (and so had its role set).
pub(crate) async fn set_role(tx: &mut DbTx, account: Uuid, role: &str) -> Result<bool, DbError> {
    Ok(sqlx::query(SET_ROLE)
        .bind(account)
        .bind(role)
        .execute(tx.conn())
        .await?
        .rows_affected()
        == 1)
}

pub(crate) async fn set_state(tx: &mut DbTx, account: Uuid, state: &str) -> Result<(), DbError> {
    sqlx::query(SET_STATE)
        .bind(account)
        .bind(state)
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

pub(crate) async fn mark_deleting(
    tx: &mut DbTx,
    account: Uuid,
    at: Timestamp,
) -> Result<(), DbError> {
    sqlx::query(MARK_DELETING)
        .bind(account)
        .bind(at.as_unix_micros())
        .execute(tx.conn())
        .await
        .map(drop)?;
    Ok(())
}

pub(crate) async fn lock_active_admins(tx: &mut DbTx) -> Result<Vec<Uuid>, DbError> {
    Ok(sqlx::query_scalar::<_, Uuid>(ACTIVE_ADMINS)
        .fetch_all(tx.conn())
        .await?)
}

pub(crate) async fn list(
    tx: &mut DbTx,
    role: Option<&str>,
    state: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ListedAccount>, DbError> {
    let rows = sqlx::query_as::<_, ListedTuple>(LIST)
        .bind(role)
        .bind(state)
        .bind(limit)
        .bind(offset)
        .fetch_all(tx.conn())
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, display_name, role, state, last_login_micros)| ListedAccount {
                id,
                display_name,
                role,
                state,
                last_login_micros,
            },
        )
        .collect())
}

/// Deletes the account row and everything of identity that belongs to it.
pub(crate) async fn delete_with_owned_rows(tx: &mut DbTx, account: Uuid) -> Result<(), DbError> {
    for sql in DELETE_OWNED.into_iter().chain([DELETE_ACCOUNT]) {
        sqlx::query(sql)
            .bind(account)
            .execute(tx.conn())
            .await
            .map(drop)?;
    }
    Ok(())
}
