//! The invite and operator services on the harness, sign-in with a setup token, and reads of the token rows and
//! of what an account owns, as the schema owner.

use contracts::{SignInError, SignInOutcome};
use sc_ids::{RawToken, TokenKind};
use sqlx::PgConnection;
use sunnycloud_identity::IdentityInvites;
use uuid::Uuid;

use super::Harness;
use super::sign_in::{CLIENT_IP, HOST, World, begin_request, params};

/// `InviteService` and `OperatorIdentityOps` on the component's pool, clock, section and audit writer.
pub fn invites(harness: &Harness) -> IdentityInvites {
    IdentityInvites::new(
        harness.api.clone(),
        harness.clock.clone(),
        &harness.config,
        harness.audit(),
    )
}

/// `begin` with `setup` as the setup token, the provider login of `subject` and `complete`.
pub async fn sign_in_with_setup(
    world: &World,
    provider: &str,
    subject: &str,
    setup: &str,
) -> Result<SignInOutcome, SignInError> {
    let mut request = begin_request(provider, "/files");
    request.setup_token = Some(RawToken::from_presented(TokenKind::Setup, setup.to_owned()));
    let redirect = world.service.begin(request).await.expect("begin");
    let callback = world.idp.authorize_with(&redirect.url, subject, |_| {});
    world
        .service
        .complete(
            provider,
            params(callback),
            &redirect.browser_binding,
            HOST,
            CLIENT_IP,
        )
        .await
}

/// One stored token row, without its timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRow {
    pub id: Uuid,
    pub kind: String,
    pub token_hash: Vec<u8>,
    pub grants_role: String,
    pub target_account_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub redeemed_by: Option<Uuid>,
    pub note: Option<String>,
}

type TokenTuple = (
    Uuid,
    String,
    Vec<u8>,
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
);

/// Every row of `invites`, oldest first.
pub async fn token_rows(conn: &mut PgConnection) -> Vec<TokenRow> {
    let rows: Vec<TokenTuple> = sqlx::query_as(
        "SELECT id, kind, token_hash, grants_role, target_account_id, created_by, redeemed_by, note \
         FROM invites ORDER BY created_at, id",
    )
    .fetch_all(conn)
    .await
    .expect("read invites");
    rows.into_iter()
        .map(|row| TokenRow {
            id: row.0,
            kind: row.1,
            token_hash: row.2,
            grants_role: row.3,
            target_account_id: row.4,
            created_by: row.5,
            redeemed_by: row.6,
            note: row.7,
        })
        .collect()
}

/// The SHA-256 of `token`, as the database computes it.
pub async fn sha256_of(conn: &mut PgConnection, token: &str) -> Vec<u8> {
    sqlx::query_scalar("SELECT sha256(convert_to($1, 'UTF8'))")
        .bind(token)
        .fetch_one(conn)
        .await
        .expect("hash the token")
}

/// `(provider, provider_user_id)` of every identity of `account`, sorted.
pub async fn identities_of(conn: &mut PgConnection, account: Uuid) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT provider, provider_user_id FROM identities WHERE account_id = $1 \
         ORDER BY provider, provider_user_id",
    )
    .bind(account)
    .fetch_all(conn)
    .await
    .expect("read identities")
}

/// `(role, state)` of `account`.
pub async fn role_and_state(conn: &mut PgConnection, account: Uuid) -> (String, String) {
    sqlx::query_as("SELECT role, state FROM accounts WHERE id = $1")
        .bind(account)
        .fetch_one(conn)
        .await
        .expect("read the account")
}

/// `(live sessions, live device tokens)` of `account`.
pub async fn live_credentials(conn: &mut PgConnection, account: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM sessions WHERE account_id = $1 AND revoked_at IS NULL), \
         (SELECT count(*) FROM device_tokens WHERE account_id = $1 AND revoked_at IS NULL)",
    )
    .bind(account)
    .fetch_one(conn)
    .await
    .expect("count live credentials")
}

/// A file node without a revision under the root of `account`; returns its id.
pub async fn file_node(conn: &mut PgConnection, account: Uuid, name: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, \
         updated_at) \
         SELECT gen_random_uuid(), $1, id, 'file', $2, $2, 1, created_at, created_at FROM nodes \
         WHERE account_id = $1 AND parent_id IS NULL RETURNING id",
    )
    .bind(account)
    .bind(name)
    .fetch_one(conn)
    .await
    .expect("insert a file node")
}

/// The ids of every node of `account`, sorted.
pub async fn nodes_of(conn: &mut PgConnection, account: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT id FROM nodes WHERE account_id = $1 ORDER BY id")
        .bind(account)
        .fetch_all(conn)
        .await
        .expect("read nodes")
}
