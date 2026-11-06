//! Fixture rows, inserted as the schema owner. Hashes are `sha256` of a label, as the tokens' stored form is.
//! Every timestamp is the bound `at` (microseconds since the epoch) from the manual clock.

use sqlx::PgConnection;
use uuid::Uuid;

pub async fn account(conn: &mut PgConnection, id: Uuid, at: i64) {
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(id)
    .bind(at)
    .execute(conn)
    .await
    .expect("insert account");
}

pub async fn identity(
    conn: &mut PgConnection,
    account: Uuid,
    provider_user_id: &str,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO identities (id, account_id, provider, provider_user_id, email_verified, created_at) \
         VALUES (gen_random_uuid(), $1, 'github', $2, false, 'epoch'::timestamptz + $3::bigint * interval '1 microsecond')",
    )
    .bind(account)
    .bind(provider_user_id)
    .bind(at)
    .execute(conn)
    .await
    .map(drop)
}

/// A session whose token and refresh hashes are derived from `token` and `refresh`.
pub async fn session(
    conn: &mut PgConnection,
    account: Uuid,
    token: &str,
    refresh: &str,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sessions (id, account_id, client_kind, label, token_hash, access_expires_at, \
         refresh_hash, refresh_expires_at, created_at) \
         VALUES (gen_random_uuid(), $1, 'web', 'browser', sha256(convert_to($2, 'UTF8')), \
         'epoch'::timestamptz + $4::bigint * interval '1 microsecond', sha256(convert_to($3, 'UTF8')), \
         'epoch'::timestamptz + $4::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + $4::bigint * interval '1 microsecond')",
    )
    .bind(account)
    .bind(token)
    .bind(refresh)
    .bind(at)
    .execute(conn)
    .await
    .map(drop)
}

pub async fn device_token(
    conn: &mut PgConnection,
    account: Uuid,
    token: &str,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO device_tokens (id, account_id, token_hash, device_name, platform, created_at) \
         VALUES (gen_random_uuid(), $1, sha256(convert_to($2, 'UTF8')), 'laptop', 'linux', \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond')",
    )
    .bind(account)
    .bind(token)
    .bind(at)
    .execute(conn)
    .await
    .map(drop)
}

/// A grant; `account` is NULL while pending.
pub async fn device_grant(
    conn: &mut PgConnection,
    account: Option<Uuid>,
    device_code: &str,
    user_code: &str,
    status: &str,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO device_grants (id, device_code_hash, user_code, requester_ip, device_name, platform, \
         status, created_at, expires_at, account_id) \
         VALUES (gen_random_uuid(), sha256(convert_to($2, 'UTF8')), $3, '203.0.113.7', 'laptop', 'linux', $4, \
         'epoch'::timestamptz + $5::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + $5::bigint * interval '1 microsecond', $1)",
    )
    .bind(account)
    .bind(device_code)
    .bind(user_code)
    .bind(status)
    .bind(at)
    .execute(conn)
    .await
    .map(drop)
}

/// An invite row of the given shape; `kind` is `invite` or `setup`.
pub async fn invite(
    conn: &mut PgConnection,
    kind: &str,
    token: &str,
    grants_role: &str,
    target_account: Option<Uuid>,
    created_by: Option<Uuid>,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO invites (id, kind, token_hash, grants_role, target_account_id, created_by, created_at, \
         expires_at) \
         VALUES (gen_random_uuid(), $1, sha256(convert_to($2, 'UTF8')), $3, $4, $5, \
         'epoch'::timestamptz + $6::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + $6::bigint * interval '1 microsecond')",
    )
    .bind(kind)
    .bind(token)
    .bind(grants_role)
    .bind(target_account)
    .bind(created_by)
    .bind(at)
    .execute(conn)
    .await
    .map(drop)
}

pub async fn oauth_state(
    conn: &mut PgConnection,
    state: &str,
    return_to: &str,
    at: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_states (state_hash, provider, nonce, pkce_verifier, return_to, client_kind, \
         created_at, expires_at) \
         VALUES (sha256(convert_to($1, 'UTF8')), 'github', 'n', 'v', $2, 'web', \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond', \
         'epoch'::timestamptz + $3::bigint * interval '1 microsecond')",
    )
    .bind(state)
    .bind(return_to)
    .bind(at)
    .execute(conn)
    .await
    .map(drop)
}

/// One row in each of the four account-scoped tables, all labelled `label`.
pub async fn scoped_rows(conn: &mut PgConnection, account: Uuid, label: &str, at: i64) {
    identity(conn, account, label, at)
        .await
        .expect("insert identity");
    session(conn, account, label, &format!("{label}-refresh"), at)
        .await
        .expect("insert session");
    device_token(conn, account, label, at)
        .await
        .expect("insert device token");
    device_grant(conn, Some(account), label, label, "approved", at)
        .await
        .expect("insert device grant");
}

/// The `(SQLSTATE, constraint)` of a failed statement.
pub fn violation(result: Result<(), sqlx::Error>) -> (String, String) {
    match result {
        Err(sqlx::Error::Database(error)) => (
            error
                .code()
                .map(|code| code.to_string())
                .unwrap_or_default(),
            error.constraint().unwrap_or_default().to_owned(),
        ),
        other => panic!("expected a database error, got {other:?}"),
    }
}
