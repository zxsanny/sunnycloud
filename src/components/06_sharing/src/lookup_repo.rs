//! The visitor's token lookup: the one place in 06 that reads `share_links` across accounts. The owner is not known
//! before the read, so it runs in system scope (ADR-014, data model 12.3); the exception is confined to
//! [`by_token_hash`]. It is one read of the UNIQUE `token_hash` index by an exact 32-byte hash, so it returns at
//! most one row and cannot be used as a scan. Every decision on the row is taken in code by the caller.

use sc_clock::Timestamp;
use sc_db::{Db, DbError, RunError};
use sc_ids::TokenHash;
use uuid::Uuid;

const BY_TOKEN_HASH: &str = "SELECT account_id, node_id, password_hash IS NOT NULL, \
    (extract(epoch FROM expires_at) * 1000000)::bigint, revoked_at IS NOT NULL \
    FROM share_links WHERE token_hash = $1";

/// The `pg_stat_activity` name of the system-scope read.
const OP: &str = "share_lookup";

/// A link as stored, without its token hash or password hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LinkRow {
    pub account_id: Uuid,
    pub node_id: Uuid,
    pub has_password: bool,
    pub expires_at: Timestamp,
    pub revoked: bool,
}

type RowTuple = (Uuid, Uuid, bool, i64, bool);

/// The link whose token hash is exactly `hash`, of any account, in any state.
pub(crate) async fn by_token_hash(
    db: &Db,
    hash: &TokenHash,
) -> Result<Option<LinkRow>, RunError<DbError>> {
    let row = db
        .run_system(OP, async |tx| {
            sqlx::query_as::<_, RowTuple>(BY_TOKEN_HASH)
                .bind(hash.as_bytes().as_slice())
                .fetch_optional(tx.conn())
                .await
                .map_err(DbError::from)
        })
        .await?;
    Ok(row.map(
        |(account_id, node_id, has_password, expires_at, revoked)| LinkRow {
            account_id,
            node_id,
            has_password,
            expires_at: Timestamp::from_unix_micros(expires_at),
            revoked,
        },
    ))
}
