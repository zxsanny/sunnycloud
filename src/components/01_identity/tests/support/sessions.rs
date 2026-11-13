//! Helpers of the session-service tests: the component on the harness, tokens from text and probes of the
//! session rows as the schema owner.

use std::sync::Arc;

use contracts::{Principal, PrincipalKind, Role};
use sc_ids::{RawToken, TokenKind};
use sqlx::PgConnection;
use sunnycloud_identity::IdentitySessions;
use uuid::Uuid;

use super::Harness;

pub fn sessions(harness: &Harness) -> IdentitySessions {
    IdentitySessions::new(
        harness.api.clone(),
        harness.clock.clone(),
        &harness.config,
        harness.audit(),
    )
}

pub fn shared_sessions(harness: &Harness) -> Arc<IdentitySessions> {
    Arc::new(sessions(harness))
}

pub fn refresh_token(text: &str) -> RawToken {
    RawToken::from_presented(TokenKind::Refresh, text.to_owned())
}

pub fn principal(account: Uuid, id: Uuid, kind: PrincipalKind) -> Principal {
    Principal {
        account_id: account,
        kind,
        session_or_device_id: id,
        role: Role::User,
        label: "test".to_owned(),
    }
}

/// Whether the session's current refresh hash is the SHA-256 of `token`.
pub async fn is_current_refresh(conn: &mut PgConnection, id: Uuid, token: &str) -> bool {
    sqlx::query_scalar(
        "SELECT refresh_hash = sha256(convert_to($2, 'UTF8')) FROM sessions WHERE id = $1",
    )
    .bind(id)
    .bind(token)
    .fetch_one(conn)
    .await
    .expect("read current refresh hash")
}

/// Whether the session's previous refresh hash is the SHA-256 of `token`.
pub async fn is_previous_refresh(conn: &mut PgConnection, id: Uuid, token: &str) -> bool {
    sqlx::query_scalar::<_, Option<bool>>(
        "SELECT prev_refresh_hash = sha256(convert_to($2, 'UTF8')) FROM sessions WHERE id = $1",
    )
    .bind(id)
    .bind(token)
    .fetch_one(conn)
    .await
    .expect("read previous refresh hash")
    .unwrap_or(false)
}

/// `(revoked, revoke_reason)` of the session.
pub async fn revocation(conn: &mut PgConnection, id: Uuid) -> (bool, Option<String>) {
    sqlx::query_as("SELECT revoked_at IS NOT NULL, revoke_reason FROM sessions WHERE id = $1")
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("read revocation")
}

/// Every session row as text, in id order: equal before and after means no column of any row changed.
pub async fn all_rows(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar("SELECT s::text FROM sessions s ORDER BY id")
        .fetch_all(conn)
        .await
        .expect("read session rows")
}

/// The transaction id that last wrote the session row: it changes with every write, even of equal values.
pub async fn row_version(conn: &mut PgConnection, id: Uuid) -> String {
    sqlx::query_scalar("SELECT xmin::text FROM sessions WHERE id = $1")
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("read row version")
}

pub async fn session_last_seen(conn: &mut PgConnection, id: Uuid) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM last_seen_at) * 1000000)::bigint FROM sessions WHERE id = $1",
    )
    .bind(id)
    .fetch_one(conn)
    .await
    .expect("read session last_seen_at")
}

pub async fn device_last_seen(conn: &mut PgConnection, id: Uuid) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT (extract(epoch FROM last_seen_at) * 1000000)::bigint FROM device_tokens WHERE id = $1",
    )
    .bind(id)
    .fetch_one(conn)
    .await
    .expect("read device last_seen_at")
}

/// The sealed successor pair of the last rotation, as stored.
pub async fn sealed_successor(conn: &mut PgConnection, id: Uuid) -> Option<Vec<u8>> {
    sqlx::query_scalar("SELECT successor_enc FROM sessions WHERE id = $1")
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("read the sealed successor")
}

/// Makes the row look as if it was rotated before the column existed.
pub async fn forget_successor(conn: &mut PgConnection, id: Uuid) {
    sqlx::query("UPDATE sessions SET successor_enc = NULL WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await
        .expect("clear the sealed successor");
}
