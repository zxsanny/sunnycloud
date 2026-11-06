//! AC-1 to AC-3 of the mobile one-time code (IT-01-14, IT-01-5 case 12; FT-P-29, FT-N-23, NFT-SEC-03): the
//! callback returns a code stored only as a hash, the code exchanges once for a mobile session, and a reused,
//! wrong-verifier, expired or unknown code all fail as `InvalidCode`. Real PostgreSQL, the manual clock.

mod support;

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    Authenticator, ClientKind, Credential, SessionService, SessionTokens, SignInError,
    SignInOutcome,
};
use sc_ids::{RawToken, TokenKind};
use sqlx::PgConnection;
use support::auth::authenticator;
use support::sessions::{refresh_token, sessions};
use support::sign_in::{CHALLENGE, OIDC, VERIFIER, World, params, world, world_with};
use support::{START, fixtures};
use uuid::Uuid;

const CODE_TTL: Duration = Duration::from_millis(500);
const SUBJECT: &str = "sub-mobile";

async fn short_lived_world() -> World {
    world_with(|config| config.login_code_ttl = CODE_TTL).await
}

fn is_invalid_code(result: &Result<SessionTokens, SignInError>) -> bool {
    matches!(result, Err(SignInError::InvalidCode))
}

async fn exchange(
    world: &World,
    code: &RawToken,
    verifier: &str,
) -> Result<SessionTokens, SignInError> {
    world.service.exchange_mobile_code(code, verifier).await
}

/// `(sessions, device tokens, login codes)`.
async fn rows(conn: &mut PgConnection) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM sessions), (SELECT count(*) FROM device_tokens), \
         (SELECT count(*) FROM login_codes)",
    )
    .fetch_one(conn)
    .await
    .expect("count rows")
}

#[tokio::test]
async fn mobile_callback_returns_a_code_and_stores_only_its_hash_with_the_challenge_and_no_session()
{
    // Arrange
    let mut world = short_lived_world().await;
    let account = world.known_identity(OIDC, SUBJECT).await;
    let mut request = support::sign_in::begin_request(OIDC, "/files");
    request.client_kind = ClientKind::Android;
    request.code_challenge = Some(CHALLENGE.to_owned());
    let redirect = world.service.begin(request).await.expect("begin");
    let callback = world.idp.authorize(&redirect.url, SUBJECT);

    // Act
    let outcome = world
        .service
        .complete(
            OIDC,
            params(callback),
            &redirect.browser_binding,
            support::sign_in::HOST,
        )
        .await
        .expect("complete");

    // Assert
    let SignInOutcome::MobileCode { code, return_to } = outcome else {
        panic!("expected a mobile code");
    };
    assert_eq!(return_to, "/files");
    assert_eq!(code.kind(), TokenKind::MobileLoginCode);
    let (hashed, owner, challenge, kind, ttl_micros, dump): (bool, Uuid, String, String, i64, String) =
        sqlx::query_as(
            "SELECT code_hash = sha256(convert_to($1, 'UTF8')), account_id, code_challenge, client_kind, \
             (extract(epoch FROM expires_at - created_at) * 1000000)::bigint, row_to_json(l)::text \
             FROM login_codes l",
        )
        .bind(code.expose())
        .fetch_one(&mut world.conn)
        .await
        .expect("exactly one login code");
    assert!(hashed, "the stored value is the SHA-256 of the code");
    assert_eq!(
        (owner, challenge.as_str(), kind.as_str()),
        (account, CHALLENGE, "android")
    );
    assert_eq!(ttl_micros, 500_000, "expiry comes from login_code_ttl");
    assert!(
        !dump.contains(code.expose()),
        "the code is not stored in the clear"
    );
    let counts = world.counts().await;
    assert_eq!(
        (counts.sessions, counts.login_codes, counts.oauth_states),
        (0, 1, 0)
    );
    let touched: bool = sqlx::query_scalar("SELECT last_login_at IS NOT NULL FROM identities")
        .fetch_one(&mut world.conn)
        .await
        .expect("identity row");
    assert!(touched, "the sign-in counts as the identity's last login");
    world.finish().await;
}

#[tokio::test]
async fn code_exchanges_once_for_a_mobile_session_with_a_rotating_refresh_token_then_is_invalid() {
    // Arrange
    let mut world = short_lived_world().await;
    let account = world.known_identity(OIDC, SUBJECT).await;
    let code = world
        .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
        .await;

    // Act
    let first = exchange(&world, &code, VERIFIER).await;
    let second = exchange(&world, &code, VERIFIER).await;

    // Assert
    let tokens = first.expect("the first exchange succeeds");
    assert!(is_invalid_code(&second), "{second:?}");
    assert_eq!(tokens.expires_in, world.harness.config.access_token_ttl);
    assert_eq!(
        rows(&mut world.conn).await,
        (1, 0, 0),
        "one session, no device token, code consumed"
    );
    let (kind, access_hashed, refresh_hashed, account_id, label): (
        String,
        bool,
        bool,
        Uuid,
        String,
    ) = sqlx::query_as(
        "SELECT client_kind, token_hash = sha256(convert_to($1, 'UTF8')), \
             refresh_hash = sha256(convert_to($2, 'UTF8')), account_id, label FROM sessions",
    )
    .bind(tokens.access.expose())
    .bind(tokens.refresh.expose())
    .fetch_one(&mut world.conn)
    .await
    .expect("session row");
    assert_eq!(
        (kind.as_str(), account_id, label.as_str()),
        ("android", account, "android")
    );
    assert!(access_hashed && refresh_hashed);
    let principal = authenticator(&world.harness)
        .authenticate(Credential::Session(tokens.access))
        .await
        .expect("the access token authenticates");
    assert_eq!(principal.account_id, account);
    let rotated = sessions(&world.harness)
        .refresh(&refresh_token(tokens.refresh.expose()))
        .await
        .expect("the refresh token rotates");
    assert_ne!(rotated.refresh.expose(), tokens.refresh.expose());
    world.finish().await;
}

#[tokio::test]
async fn an_ios_code_mints_an_ios_session() {
    // Arrange
    let mut world = world().await;
    world.known_identity(OIDC, SUBJECT).await;
    let code = world.mobile_code(ClientKind::Ios, SUBJECT, CHALLENGE).await;

    // Act
    let result = exchange(&world, &code, VERIFIER).await;

    // Assert
    result.expect("exchange");
    let kind: String = sqlx::query_scalar("SELECT client_kind FROM sessions")
        .fetch_one(&mut world.conn)
        .await
        .expect("session row");
    assert_eq!(kind, "ios");
    world.finish().await;
}

#[tokio::test]
async fn wrong_verifier_is_invalid_consumes_the_code_and_mints_nothing() {
    // Arrange
    let mut world = world().await;
    world.known_identity(OIDC, SUBJECT).await;
    let other = Uuid::new_v4();
    fixtures::account(&mut world.conn, other, START.as_unix_micros()).await;
    fixtures::login_code(
        &mut world.conn,
        other,
        "other-account-code",
        START.as_unix_micros() + 60_000_000,
    )
    .await;
    let code = world
        .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
        .await;
    let before_other: i64 =
        sqlx::query_scalar("SELECT count(*) FROM login_codes WHERE account_id = $1")
            .bind(other)
            .fetch_one(&mut world.conn)
            .await
            .expect("count");

    // Act
    let wrong = exchange(&world, &code, "not-the-verifier-of-this-code").await;
    let right_afterwards = exchange(&world, &code, VERIFIER).await;

    // Assert
    assert!(is_invalid_code(&wrong), "{wrong:?}");
    assert!(
        is_invalid_code(&right_afterwards),
        "the code was consumed by the failed attempt"
    );
    assert_eq!(before_other, 1);
    assert_eq!(
        rows(&mut world.conn).await,
        (0, 0, 1),
        "only the other account's code remains"
    );
    world.finish().await;
}

#[tokio::test]
async fn expired_code_is_invalid_consumed_and_mints_nothing() {
    // Arrange
    let mut world = short_lived_world().await;
    world.known_identity(OIDC, SUBJECT).await;
    let code = world
        .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
        .await;
    world.harness.clock.tick(CODE_TTL);

    // Act
    let result = exchange(&world, &code, VERIFIER).await;

    // Assert
    assert!(is_invalid_code(&result), "{result:?}");
    assert_eq!(rows(&mut world.conn).await, (0, 0, 0));
    world.finish().await;
}

#[tokio::test]
async fn code_is_still_good_just_before_its_expiry() {
    // Arrange
    let mut world = short_lived_world().await;
    world.known_identity(OIDC, SUBJECT).await;
    let code = world
        .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
        .await;
    world
        .harness
        .clock
        .tick(CODE_TTL - Duration::from_millis(1));

    // Act
    let result = exchange(&world, &code, VERIFIER).await;

    // Assert
    result.expect("a code inside its ttl exchanges");
    world.finish().await;
}

#[tokio::test]
async fn unknown_code_is_the_same_invalid_code_as_a_reused_one() {
    // Arrange
    let mut world = world().await;
    world.known_identity(OIDC, SUBJECT).await;
    let unknown = RawToken::from_presented(TokenKind::MobileLoginCode, "never-issued".to_owned());

    // Act
    let result = exchange(&world, &unknown, VERIFIER).await;

    // Assert
    assert!(is_invalid_code(&result), "{result:?}");
    assert_eq!(rows(&mut world.conn).await, (0, 0, 0));
    world.finish().await;
}

#[tokio::test]
async fn code_of_an_account_disabled_after_sign_in_is_invalid_and_consumed() {
    // Arrange
    let mut world = world().await;
    let account = world.known_identity(OIDC, SUBJECT).await;
    let code = world
        .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
        .await;
    fixtures::set_account_state(&mut world.conn, account, "disabled").await;

    // Act
    let result = exchange(&world, &code, VERIFIER).await;

    // Assert
    assert!(is_invalid_code(&result), "{result:?}");
    assert_eq!(rows(&mut world.conn).await, (0, 0, 0));
    world.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_exchanges_of_one_code_mint_exactly_one_session() {
    // Arrange
    const PARALLEL: usize = 8;
    let mut world = world().await;
    world.known_identity(OIDC, SUBJECT).await;
    let code = Arc::new(
        world
            .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
            .await,
    );
    let service = Arc::new(
        sunnycloud_identity::IdentitySignIn::new(
            world.harness.api.clone(),
            world.harness.clock.clone(),
            &world.harness.config,
        )
        .expect("service"),
    );
    let gate = Arc::new(tokio::sync::Barrier::new(PARALLEL));

    // Act
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..PARALLEL {
        let (service, code, gate) = (Arc::clone(&service), Arc::clone(&code), Arc::clone(&gate));
        tasks.spawn(async move {
            gate.wait().await;
            service.exchange_mobile_code(&code, VERIFIER).await
        });
    }
    let mut results = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        results.push(joined.expect("task"));
    }

    // Assert
    let minted = results.iter().filter(|result| result.is_ok()).count();
    let invalid = results
        .iter()
        .filter(|result| is_invalid_code(result))
        .count();
    assert_eq!((minted, invalid), (1, PARALLEL - 1), "{results:?}");
    assert_eq!(rows(&mut world.conn).await, (1, 0, 0));
    drop(service);
    world.finish().await;
}
