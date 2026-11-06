//! AC-2 and AC-76 (IT-01-11, FT-N-45, NFT-SEC-49): pending, too-fast and denied polls give nothing, and a
//! device code exchanges for a token exactly once, under real parallel polls. Real PostgreSQL, manual clock.

mod support;

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    Authenticator, Credential, Decision, DeviceGrantError, DeviceGrantService, PrincipalKind,
};
use sc_ids::{RawToken, TokenKind};
use sqlx::Connection;
use support::Harness;
use support::auth::authenticator;
use support::device::{
    POLL_INTERVAL, approve, code_again, geo, grant_status, grants, ip, member, shared_grants,
    token_count,
};
use tokio::sync::Barrier;

const PARALLEL_POLLS: usize = 20;
const ROUNDS: usize = 5;

#[tokio::test]
async fn pending_too_fast_and_denied_polls_mint_nothing() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, geo(&[]));
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");

    // Act
    let first = service.poll(&authorization.device_code).await;
    harness.clock.tick(POLL_INTERVAL / 2);
    let too_fast = service.poll(&authorization.device_code).await;
    let view = service
        .lookup_for_approval(&approver, &authorization.user_code, ip("10.0.0.6"))
        .await
        .expect("lookup");
    service
        .decide(&approver, view.grant_id, Decision::Deny)
        .await
        .expect("deny");
    harness.clock.tick(POLL_INTERVAL / 2);
    let denied = service.poll(&authorization.device_code).await;
    let late_approval = service
        .decide(&approver, view.grant_id, Decision::Approve)
        .await;
    let denied_again = service.poll(&authorization.device_code).await;

    // Assert
    assert!(matches!(first, Err(DeviceGrantError::AuthorizationPending)));
    assert!(matches!(too_fast, Err(DeviceGrantError::SlowDown)));
    assert!(matches!(denied, Err(DeviceGrantError::AccessDenied)));
    assert!(matches!(
        late_approval,
        Err(DeviceGrantError::AlreadyDecided)
    ));
    assert!(matches!(denied_again, Err(DeviceGrantError::AccessDenied)));
    assert_eq!(token_count(&mut conn).await, 0);
    assert_eq!(
        grant_status(&mut conn, &authorization.device_code).await,
        "denied"
    );
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn a_poll_exactly_one_interval_after_the_last_answer_is_answered() {
    // Arrange
    let mut harness = Harness::new().await;
    let service = grants(&mut harness, geo(&[]));
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    service
        .poll(&authorization.device_code)
        .await
        .expect_err("pending");

    // Act
    harness.clock.tick(POLL_INTERVAL - Duration::from_micros(1));
    let just_early = service.poll(&authorization.device_code).await;
    harness.clock.tick(Duration::from_micros(1));
    let on_time = service.poll(&authorization.device_code).await;
    harness.clock.tick(POLL_INTERVAL / 2);
    let early_again = service.poll(&authorization.device_code).await;

    // Assert
    assert!(matches!(just_early, Err(DeviceGrantError::SlowDown)));
    assert!(matches!(
        on_time,
        Err(DeviceGrantError::AuthorizationPending)
    ));
    assert!(matches!(early_again, Err(DeviceGrantError::SlowDown)));
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn an_unknown_device_code_is_refused_like_an_expired_one() {
    // Arrange
    let mut harness = Harness::new().await;
    let service = grants(&mut harness, geo(&[]));
    let unknown = RawToken::from_presented(TokenKind::DeviceCode, "never-issued".to_owned());

    // Act
    let result = service.poll(&unknown).await;

    // Assert
    assert!(matches!(result, Err(DeviceGrantError::ExpiredToken)));
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_polls_of_an_approved_grant_mint_exactly_one_token() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = shared_grants(&mut harness, geo(&[]));
    let authenticator = authenticator(&harness);

    for round in 1..=ROUNDS {
        let authorization = service
            .request("lab-1", "linux", ip("10.0.0.5"))
            .await
            .expect("request");
        approve(
            &service,
            &approver,
            &authorization.user_code,
            ip("10.0.0.6"),
        )
        .await;
        let before = token_count(&mut conn).await;
        let barrier = Arc::new(Barrier::new(PARALLEL_POLLS));

        // Act
        let polls: Vec<_> = (0..PARALLEL_POLLS)
            .map(|_| {
                let (service, barrier) = (service.clone(), barrier.clone());
                let code = code_again(&authorization.device_code);
                tokio::spawn(async move {
                    barrier.wait().await;
                    service.poll(&code).await
                })
            })
            .collect();
        let mut results = Vec::new();
        for poll in polls {
            results.push(poll.await.expect("poll task"));
        }
        let later = service.poll(&authorization.device_code).await;

        // Assert
        let tokens: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(
            tokens.len(),
            1,
            "round {round}: exactly one poll gets the token"
        );
        assert!(
            results
                .iter()
                .filter_map(|r| r.as_ref().err())
                .all(|error| matches!(error, DeviceGrantError::ExpiredToken)),
            "round {round}: the others fail as expired: {results:?}"
        );
        assert!(matches!(later, Err(DeviceGrantError::ExpiredToken)));
        assert_eq!(token_count(&mut conn).await, before + 1, "round {round}");
        assert_eq!(
            grant_status(&mut conn, &authorization.device_code).await,
            "consumed"
        );
        let principal = authenticator
            .authenticate(Credential::Device(RawToken::from_presented(
                TokenKind::DeviceToken,
                tokens[0].0.expose().to_owned(),
            )))
            .await
            .expect("the minted token authenticates");
        assert_eq!(principal.kind, PrincipalKind::Device);
        assert_eq!(principal.account_id, approver.account_id);
        assert_eq!(principal.label, "lab-1");
    }
    conn.close().await.expect("close fixture connection");
    drop((service, authenticator));
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_code_that_expired_before_approval_never_mints() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = shared_grants(&mut harness, geo(&[]));
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let view = service
        .lookup_for_approval(&approver, &authorization.user_code, ip("10.0.0.6"))
        .await
        .expect("lookup before expiry");
    harness.clock.tick(harness.config.device_code_ttl);
    let barrier = Arc::new(Barrier::new(PARALLEL_POLLS));

    // Act
    let approval = service
        .decide(&approver, view.grant_id, Decision::Approve)
        .await;
    let polls: Vec<_> = (0..PARALLEL_POLLS)
        .map(|_| {
            let (service, barrier) = (service.clone(), barrier.clone());
            let code = code_again(&authorization.device_code);
            tokio::spawn(async move {
                barrier.wait().await;
                service.poll(&code).await
            })
        })
        .collect();
    let mut results = Vec::new();
    for poll in polls {
        results.push(poll.await.expect("poll task"));
    }

    // Assert
    assert!(matches!(approval, Err(DeviceGrantError::Expired)));
    assert!(
        results
            .iter()
            .all(|result| matches!(result, Err(DeviceGrantError::ExpiredToken))),
        "{results:?}"
    );
    assert_eq!(token_count(&mut conn).await, 0);
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
