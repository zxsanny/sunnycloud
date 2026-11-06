//! The batch deletes of the identity janitor. They span every account, so they run on the system-scope
//! transaction opened by `auth_repo`; one call is one batch of at most `limit` rows. A row is expired when its
//! boundary is at or before the cutoff the caller binds (the clock, never the database clock).

use sc_clock::Timestamp;
use sc_db::{DbError, DbTx};

/// The tables the janitor empties, each by its own boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    OauthStates,
    LoginCodes,
    DeviceGrants,
    Sessions,
}

const OAUTH_STATES: &str = "DELETE FROM oauth_states WHERE state_hash IN \
    (SELECT state_hash FROM oauth_states \
     WHERE expires_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' LIMIT $2)";

const LOGIN_CODES: &str = "DELETE FROM login_codes WHERE code_hash IN \
    (SELECT code_hash FROM login_codes \
     WHERE expires_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' LIMIT $2)";

// A grant is consumed only before it expires, so the retention after expiry also covers the retention after
// consumption: the table keeps no consumption time.
const DEVICE_GRANTS: &str = "DELETE FROM device_grants WHERE id IN \
    (SELECT id FROM device_grants \
     WHERE expires_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' LIMIT $2)";

const SESSIONS: &str = "DELETE FROM sessions WHERE id IN \
    (SELECT id FROM sessions \
     WHERE revoked_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' \
     OR refresh_expires_at <= 'epoch'::timestamptz + $1::bigint * interval '1 microsecond' LIMIT $2)";

/// Deletes up to `limit` rows of `target` whose boundary is at or before `cutoff`; returns how many.
pub(crate) async fn delete_batch(
    tx: &mut DbTx,
    target: Target,
    cutoff: Timestamp,
    limit: i64,
) -> Result<u64, DbError> {
    let sql = match target {
        Target::OauthStates => OAUTH_STATES,
        Target::LoginCodes => LOGIN_CODES,
        Target::DeviceGrants => DEVICE_GRANTS,
        Target::Sessions => SESSIONS,
    };
    Ok(sqlx::query(sql)
        .bind(cutoff.as_unix_micros())
        .bind(limit)
        .execute(tx.conn())
        .await?
        .rows_affected())
}
