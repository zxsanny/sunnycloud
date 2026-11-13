//! AZ-3691 AC-3 and the sign-in half of AC-4 (IT-01-13, NFT-SEC-04, AC-20), with the owner decisions of
//! 2026-11-13: every sign-in completion that ends without a session counts as a failed step (10 per window per
//! address), invalid tokens presented at sign-in count separately (100 per window per address), each trip writes
//! one `abuse_trip` event and one WARN line without the address, and other addresses are not limited. Real
//! `AbuseLimiter` with windows scaled to milliseconds, real PostgreSQL, manual clock, the in-process provider.

mod support;

use std::net::IpAddr;
use std::time::Duration;

use contracts::{BrowserBinding, CallbackParams, SignInError, SignInOutcome};
use sc_ids::{RawToken, TokenKind};
use sc_ratelimit::{LimitId, LimitSpec, LimitTable};
use support::audit;
use support::logs::LogCapture;
use support::sign_in::{CLIENT_IP, HOST, OIDC, VERIFIER, World, params, world_limited};
use tracing::Level;

const WINDOW: Duration = Duration::from_millis(500);
const UNUSED: LimitSpec = LimitSpec::new(1_000, Duration::from_secs(60));
const KNOWN: &str = "known-subject";

fn other_ip() -> IpAddr {
    "203.0.113.50".parse().expect("test address")
}

/// AC-20's numbers over a 500 ms window.
async fn world() -> World {
    let table = LimitTable::new(UNUSED, LimitSpec::new(100, WINDOW))
        .with_spec(LimitId::SigninIp, LimitSpec::new(10, WINDOW));
    let mut world = world_limited(|_| {}, table).await;
    world.known_identity(OIDC, KNOWN).await;
    world
}

/// `begin` from `CLIENT_IP` and the provider login of `subject`: the callback and its binding.
async fn started(world: &World, subject: &str) -> (CallbackParams, BrowserBinding) {
    let (url, binding) = world.begin(OIDC).await;
    (params(world.idp.authorize(&url, subject)), binding)
}

async fn complete(
    world: &World,
    (callback, binding): (CallbackParams, BrowserBinding),
    from: IpAddr,
) -> Result<SignInOutcome, SignInError> {
    world
        .service
        .complete(OIDC, callback, &binding, HOST, from)
        .await
}

fn limited<T: std::fmt::Debug>(result: &Result<T, SignInError>) -> bool {
    matches!(result, Err(SignInError::RateLimited { retry_after }) if !retry_after.is_zero())
}

#[tokio::test]
async fn the_eleventh_failed_step_from_one_address_is_limited_once_audited_and_other_addresses_and_the_next_window_are_not()
 {
    // Arrange
    let mut world = world().await;
    let mut attempts = Vec::new();
    for index in 0..11 {
        attempts.push(started(&world, &format!("stranger-{index}")).await);
    }
    let spare = started(&world, KNOWN).await;
    let logs = LogCapture::install();

    // Act
    let mut results = Vec::new();
    for (index, (callback, binding)) in attempts.into_iter().enumerate() {
        // Half of the failures are foreign bindings (untrusted state), half unknown identities without an
        // invite (invitation required): both end without a session.
        let binding = if index % 2 == 0 {
            BrowserBinding("not-the-cookie".to_owned())
        } else {
            binding
        };
        results.push(complete(&world, (callback, binding), CLIENT_IP).await);
    }
    let other_address = complete(&world, spare, other_ip()).await;
    world.harness.clock.tick(WINDOW);
    let next_window = complete(&world, started(&world, KNOWN).await, CLIENT_IP).await;

    // Assert
    assert!(
        results[..10].iter().all(|result| matches!(
            result,
            Err(SignInError::UntrustedState | SignInError::InvitationRequired)
        )),
        "{results:?}"
    );
    assert!(limited(&results[10]), "{:?}", results[10]);
    assert!(
        matches!(other_address, Ok(SignInOutcome::Session { .. })),
        "{other_address:?}"
    );
    assert!(
        matches!(next_window, Ok(SignInOutcome::Session { .. })),
        "{next_window:?}"
    );
    let trips = audit::of(&mut world.conn, "abuse_trip").await;
    assert_eq!(trips.len(), 1, "{trips:?}");
    assert_eq!(
        trips[0].detail.as_deref(),
        Some(r#"{"limit": "signin_ip"}"#)
    );
    assert_eq!(trips[0].ip, Some(CLIENT_IP.to_string()));
    let warns = logs.at(Level::WARN, "abuse limit tripped");
    assert_eq!(warns.len(), 1, "{:?}", logs.lines());
    assert_eq!(
        warns[0].text, " message=abuse limit tripped limit=\"signin_ip\"",
        "the limit only: no address, token or code"
    );
    world.finish().await;
}

#[tokio::test]
async fn the_hundred_and_first_invalid_mobile_code_from_one_address_is_limited_and_audited_once() {
    // Arrange
    let mut world = world().await;
    let code = |index: usize| {
        RawToken::from_presented(TokenKind::MobileLoginCode, format!("no-such-code-{index}"))
    };

    // Act
    let mut results = Vec::new();
    for index in 0..102 {
        let result = world
            .service
            .exchange_mobile_code(&code(index), VERIFIER, CLIENT_IP)
            .await;
        results.push(result);
    }
    let other_address = world
        .service
        .exchange_mobile_code(&code(999), VERIFIER, other_ip())
        .await;

    // Assert
    assert!(
        results[..100]
            .iter()
            .all(|result| matches!(result, Err(SignInError::InvalidCode))),
        "{results:?}"
    );
    assert!(results[100..].iter().all(limited), "{:?}", &results[100..]);
    assert!(
        matches!(other_address, Err(SignInError::InvalidCode)),
        "{other_address:?}"
    );
    let trips = audit::of(&mut world.conn, "abuse_trip").await;
    let details: Vec<Option<String>> = trips.into_iter().map(|row| row.detail).collect();
    assert_eq!(
        details,
        vec![Some(r#"{"limit": "invalid_token_ip"}"#.to_owned())]
    );
    world.finish().await;
}

#[tokio::test]
async fn while_an_address_is_limited_even_a_valid_begin_is_refused() {
    // Arrange
    let world = world().await;
    for index in 0..10 {
        let (callback, _) = started(&world, &format!("stranger-{index}")).await;
        let foreign = BrowserBinding("not-the-cookie".to_owned());
        complete(&world, (callback, foreign), CLIENT_IP)
            .await
            .expect_err("foreign binding");
    }

    // Act
    let begin = world
        .service
        .begin(support::sign_in::begin_request(OIDC, "/files"))
        .await;

    // Assert
    assert!(limited(&begin), "{begin:?}");
    world.finish().await;
}
