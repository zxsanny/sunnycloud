//! Credentials made through the ports only (ST-01-1, ST-01-4, ST-01-5, AT-01-1): the setup token signs the first
//! admin in, the admin's member invite signs another identity in, further sign-ins of a known identity add
//! sessions, and the device grant (request, lookup and approval by the account's own session, poll) adds device
//! tokens. Also a dump of every row of every table, as the schema owner, for the "no row touched" and "no
//! plaintext at rest" probes.

use contracts::{
    Authenticator, InviteService, OperatorIdentityOps, Principal, SessionTokens, SignInError,
    SignInOutcome,
};
use sqlx::{AssertSqlSafe, PgConnection};
use sunnycloud_identity::{IdentityAuthenticator, IdentityInvites};
use uuid::Uuid;

use super::auth::session;
use super::invites::sign_in_with_setup;
use super::registration::{Rig, sign_in};
use super::sign_in::OIDC;

/// The session pair of a sign-in that must have produced one.
pub fn session_tokens(outcome: Result<SignInOutcome, SignInError>) -> SessionTokens {
    match outcome {
        Ok(SignInOutcome::Session { tokens, .. }) => tokens,
        other => panic!("expected a session, got {other:?}"),
    }
}

/// `ensure_setup_token`, then `subject` signs in with it: the first admin's session.
pub async fn first_admin(rig: &Rig, operator: &IdentityInvites, subject: &str) -> SessionTokens {
    let setup = operator
        .ensure_setup_token()
        .await
        .expect("ensure the setup token")
        .expect("a setup token while no account exists");
    session_tokens(sign_in_with_setup(&rig.world, OIDC, subject, setup.expose()).await)
}

/// The admin creates a member invite and `subject` signs in with it: the member's session.
pub async fn member_by_invite(
    rig: &Rig,
    operator: &IdentityInvites,
    admin: Uuid,
    subject: &str,
) -> SessionTokens {
    let invite = operator
        .create_invite(admin, "member")
        .await
        .expect("create a member invite");
    session_tokens(sign_in(&rig.world, OIDC, subject, Some(invite.0.expose()), |_| {}).await)
}

/// A further sign-in of the known identity `subject`: another session of its account.
pub async fn another_session(rig: &Rig, subject: &str) -> SessionTokens {
    session_tokens(sign_in(&rig.world, OIDC, subject, None, |_| {}).await)
}

/// The principal the access token of `tokens` authenticates as.
pub async fn principal_of(
    authenticator: &IdentityAuthenticator,
    tokens: &SessionTokens,
) -> Principal {
    authenticator
        .authenticate(session(tokens.access.expose()))
        .await
        .expect("the session authenticates")
}

/// The base tables of the public schema, quoted for SQL, sorted.
async fn tables(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT quote_ident(table_name::text) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE' ORDER BY 1",
    )
    .fetch_all(conn)
    .await
    .expect("list the tables")
}

/// Every row of every table as `table: row text`, sorted; every column of every row is in the text, so a column
/// added later is dumped as well. With `versions`, each line also carries the row's `xmin`, which changes with
/// every write, even one of equal values.
pub async fn all_rows(conn: &mut PgConnection, versions: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for table in tables(conn).await {
        let version = if versions { "t.xmin::text" } else { "''" };
        // `table` comes from the catalog through `quote_ident`, `version` is one of two literals.
        let rows: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT {version} || ' ' || t::text FROM {table} t ORDER BY 1"
        )))
        .fetch_all(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("dump {table}: {error}"));
        lines.extend(rows.into_iter().map(|row| format!("{table}: {row}")));
    }
    lines
}
