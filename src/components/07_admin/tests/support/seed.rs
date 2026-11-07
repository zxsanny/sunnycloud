//! Fixtures and probes as the schema owner. The statements are test code (the SQL gate exempts `tests/`).

use sqlx::PgConnection;
use uuid::Uuid;

use super::harness::START;

pub const DAY_MICROS: i64 = 86_400_000_000;

pub async fn account(
    conn: &mut PgConnection,
    id: Uuid,
    name: &str,
    role: &str,
    state: &str,
    created_micros: i64,
) {
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, $2, 'epoch'::timestamptz + $3::bigint * interval '1 microsecond', $4, $5)",
    )
    .bind(id)
    .bind(state)
    .bind(created_micros)
    .bind(name)
    .bind(role)
    .execute(conn)
    .await
    .expect("insert account");
}

pub async fn quota(
    conn: &mut PgConnection,
    id: Uuid,
    limit: Option<i64>,
    used: i64,
    reserved: i64,
) {
    sqlx::query(
        "INSERT INTO quota (account_id, limit_bytes, used_bytes, reserved_bytes) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(limit)
    .bind(used)
    .bind(reserved)
    .execute(conn)
    .await
    .expect("insert quota row");
}

pub async fn identity(
    conn: &mut PgConnection,
    account: Uuid,
    email: &str,
    last_login: Option<i64>,
) {
    sqlx::query(
        "INSERT INTO identities (id, account_id, provider, provider_user_id, email, email_verified, \
         created_at, last_login_at) \
         VALUES ($1, $2, 'github', $1::text, $3, true, \
         'epoch'::timestamptz + $4::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + $5::bigint * interval '1 microsecond')",
    )
    .bind(Uuid::new_v4())
    .bind(account)
    .bind(email)
    .bind(START.as_unix_micros())
    .bind(last_login)
    .execute(conn)
    .await
    .expect("insert identity");
}

pub async fn session(conn: &mut PgConnection, account: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sessions (id, account_id, client_kind, label, token_hash, access_expires_at, \
         refresh_hash, refresh_expires_at, created_at) \
         VALUES ($1, $2, 'web', 'fixture', sha256(($1::text || 'a')::bytea), \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond', sha256(($1::text || 'r')::bytea), \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
    )
    .bind(id)
    .bind(account)
    .bind(START.as_unix_micros() + DAY_MICROS)
    .bind(START.as_unix_micros())
    .execute(conn)
    .await
    .expect("insert session");
    id
}

pub async fn device_token(conn: &mut PgConnection, account: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO device_tokens (id, account_id, token_hash, device_name, platform, created_at) \
         VALUES ($1, $2, sha256(($1::text || 'd')::bytea), 'fixture', 'linux', \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond')",
    )
    .bind(id)
    .bind(account)
    .bind(START.as_unix_micros())
    .execute(conn)
    .await
    .expect("insert device token");
    id
}

/// An ordinary user with a quota row, two sessions and one device token.
pub async fn user_with_credentials(conn: &mut PgConnection, id: Uuid, name: &str, email: &str) {
    account(conn, id, name, "user", "active", START.as_unix_micros()).await;
    quota(conn, id, None, 0, 0).await;
    identity(conn, id, email, Some(START.as_unix_micros())).await;
    session(conn, id).await;
    session(conn, id).await;
    device_token(conn, id).await;
}

/// An active admin with a quota row.
pub async fn admin(conn: &mut PgConnection, id: Uuid) {
    account(
        conn,
        id,
        "Admin Fixture",
        "admin",
        "active",
        START.as_unix_micros(),
    )
    .await;
    quota(conn, id, None, 0, 0).await;
}

pub async fn role_and_state(conn: &mut PgConnection, id: Uuid) -> (String, String) {
    sqlx::query_as("SELECT role, state FROM accounts WHERE id = $1")
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("read account")
}

/// Sessions and device tokens of the account that are revoked.
pub async fn revoked_credentials(conn: &mut PgConnection, id: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM sessions WHERE account_id = $1 AND revoked_at IS NOT NULL), \
         (SELECT count(*) FROM device_tokens WHERE account_id = $1 AND revoked_at IS NOT NULL)",
    )
    .bind(id)
    .fetch_one(conn)
    .await
    .expect("count revoked credentials")
}

pub async fn limit_of(conn: &mut PgConnection, id: Uuid) -> Option<i64> {
    sqlx::query_scalar("SELECT limit_bytes FROM quota WHERE account_id = $1")
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("read limit")
}

pub async fn row_count(conn: &mut PgConnection, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(conn)
        .await
        .expect("count rows")
}

pub struct AuditRow {
    pub event: String,
    pub actor_account_id: Option<Uuid>,
    pub actor_session_id: Option<Uuid>,
    pub subject_type: Option<String>,
    pub subject_id: Option<Uuid>,
    pub action: Option<String>,
    pub whole_row: String,
}

pub async fn audit_rows(conn: &mut PgConnection) -> Vec<AuditRow> {
    type Raw = (
        String,
        Option<Uuid>,
        Option<Uuid>,
        Option<String>,
        Option<Uuid>,
        Option<String>,
        String,
    );
    let rows: Vec<Raw> = sqlx::query_as(
        "SELECT event, actor_account_id, actor_session_id, subject_type, subject_id, detail->>'action', \
         row_to_json(audit_log)::text FROM audit_log ORDER BY id",
    )
    .fetch_all(conn)
    .await
    .expect("read audit rows");
    rows.into_iter()
        .map(|row| AuditRow {
            event: row.0,
            actor_account_id: row.1,
            actor_session_id: row.2,
            subject_type: row.3,
            subject_id: row.4,
            action: row.5,
            whole_row: row.6,
        })
        .collect()
}
