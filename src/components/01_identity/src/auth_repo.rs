//! The credential probes of the authenticator. Each is one statement: a read on a unique `*_hash` index
//! (authenticate) or on the primary key (revalidate), joined to the account. Expiry, revocation and account
//! state are returned as columns and decided in code, never in the `WHERE` clause, so every dead credential
//! costs the same database work.

use sc_db::{Db, DbError, RunError, Scope};
use sc_ids::TokenHash;
use uuid::Uuid;

use contracts::PrincipalKind;

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
