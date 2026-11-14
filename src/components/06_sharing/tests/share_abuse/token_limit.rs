//! AZ-3701 AC-1 (IT-06-8 invalid tokens, ST-06-3): `share_invalid_token_limit` unknown tokens per address per
//! minute on `open`, `authorize_content`, `authorize_derived` (AZ-4310 AC-3) and `verify_password`, one counter for
//! the four; past it the address gets `RateLimited` with a retry time on every token, a live one included, other
//! addresses are not affected, and the address is free again one window later.

use std::time::Duration;

use contracts::{PublicShareError, PublicShareService, ShareOpen};
use sc_ratelimit::LimitId;

use crate::rig::{IP1, IP2, Rig, unknown};
use crate::visitor_support::PASSWORD;

/// The invalid-token window (fixed at one minute, no key); moved on the manual clock, never slept.
const MINUTE: Duration = Duration::from_secs(60);

#[tokio::test]
async fn open_limits_invalid_tokens_per_address_and_frees_it_after_the_window() {
    // Arrange
    let rig = Rig::new().await;
    let live = rig.link(None).await;
    let limit = usize::try_from(rig.harness.config.invalid_token_limit).expect("fits");
    assert_eq!(limit, 100, "the AC-20 number");

    // Act
    let mut first = Vec::new();
    let mut other = Vec::new();
    for n in 0..limit {
        first.push(rig.shares.open(&unknown(n), IP1).await);
        if n % 2 == 0 {
            other.push(rig.shares.open(&unknown(n), IP2).await);
        }
    }
    let over = rig.shares.open(&unknown(limit), IP1).await;
    let live_while_limited = rig.shares.open(&live.token, IP1).await;
    let live_elsewhere = rig.shares.open(&live.token, IP2).await;
    rig.harness.clock.tick(MINUTE);
    let after = rig.shares.open(&unknown(limit + 1), IP1).await;

    // Assert
    for answer in first.iter().chain(&other) {
        assert!(matches!(answer, Ok(ShareOpen::NotFound)), "{answer:?}");
    }
    let Err(PublicShareError::RateLimited { retry_after }) = over else {
        panic!("the 101st call is limited: {over:?}");
    };
    assert!(retry_after > Duration::ZERO && retry_after <= MINUTE);
    assert!(matches!(
        live_while_limited,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert!(matches!(live_elsewhere, Ok(ShareOpen::Available { .. })));
    assert!(matches!(after, Ok(ShareOpen::NotFound)), "{after:?}");
    let counts = rig.harness.limiter.stats().get(LimitId::ShareTokenIp);
    assert_eq!(
        (counts.tripped, counts.limited),
        (1, 2),
        "IP1 tripped once, IP2 never"
    );
    rig.finish().await;
}

#[tokio::test]
async fn the_content_and_password_routes_count_on_the_same_address_counter_as_open() {
    // Arrange
    let rig = Rig::new().await;
    let protected = rig.link(Some(PASSWORD)).await;
    let limit = usize::try_from(rig.harness.config.invalid_token_limit).expect("fits");
    let third = limit / 3;

    // Act
    let mut answers = Vec::new();
    for n in 0..third {
        let answer = rig.shares.open(&unknown(n), IP1).await;
        answers.push(match answer {
            Ok(ShareOpen::NotFound) => Err(PublicShareError::NotFound),
            other => other.map(|_| ()),
        });
    }
    for n in third..2 * third {
        let answer = rig
            .shares
            .authorize_content(&unknown(n), None, IP1, true)
            .await;
        answers.push(answer.map(|_| ()));
    }
    for n in 2 * third..limit {
        let answer = rig.shares.verify_password(&unknown(n), PASSWORD, IP1).await;
        answers.push(answer.map(|_| ()));
    }
    let open_over = rig.shares.open(&unknown(limit), IP1).await;
    let content_over = rig
        .shares
        .authorize_content(&protected.token, None, IP1, false)
        .await;
    let password_over = rig
        .shares
        .verify_password(&protected.token, PASSWORD, IP1)
        .await;

    // Assert
    assert_eq!(answers.len(), limit);
    for answer in &answers {
        assert!(
            matches!(answer, Err(PublicShareError::NotFound)),
            "{answer:?}"
        );
    }
    assert!(matches!(
        open_over,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert!(matches!(
        content_over,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert!(matches!(
        password_over,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert_eq!(
        rig.shares.password_checks_run(),
        0,
        "no Argon2 for a limited address"
    );
    rig.finish().await;
}

#[tokio::test]
async fn the_derived_route_counts_unknown_tokens_on_the_same_address_counter_as_open() {
    // Arrange
    let rig = Rig::new().await;
    let live = rig.link(None).await;
    let limit = usize::try_from(rig.harness.config.invalid_token_limit).expect("fits");
    let half = limit / 2;
    for n in 0..half {
        let answer = rig.shares.open(&unknown(n), IP1).await;
        assert!(matches!(answer, Ok(ShareOpen::NotFound)), "{answer:?}");
    }

    // Act
    let mut derived = Vec::new();
    for n in half..limit {
        derived.push(rig.shares.authorize_derived(&unknown(n), None, IP1).await);
    }
    let derived_over = rig.shares.authorize_derived(&live.token, None, IP1).await;
    let open_over = rig.shares.open(&unknown(limit), IP1).await;
    let elsewhere = rig.shares.authorize_derived(&live.token, None, IP2).await;

    // Assert
    assert_eq!(derived.len(), limit - half);
    for answer in &derived {
        assert!(
            matches!(answer, Err(PublicShareError::NotFound)),
            "{answer:?}"
        );
    }
    assert!(matches!(
        derived_over,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert!(matches!(
        open_over,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert!(elsewhere.is_ok(), "{elsewhere:?}");
    let counts = rig.harness.limiter.stats().get(LimitId::ShareTokenIp);
    assert_eq!((counts.tripped, counts.limited), (1, 2));
    rig.finish().await;
}

#[tokio::test]
async fn valid_tokens_do_not_count_against_the_address() {
    // Arrange
    let rig = Rig::new().await;
    let plain = rig.link(None).await;
    let protected = rig.link(Some(PASSWORD)).await;
    let limit = usize::try_from(rig.harness.config.invalid_token_limit).expect("fits");

    // Act
    let mut answers = Vec::new();
    for _ in 0..=limit {
        answers.push(rig.shares.open(&plain.token, IP1).await.map(|_| ()));
        answers.push(rig.shares.open(&protected.token, IP1).await.map(|_| ()));
        let content = rig
            .shares
            .authorize_content(&plain.token, None, IP1, false)
            .await;
        answers.push(content.map(|_| ()));
    }

    // Assert
    assert!(answers.iter().all(Result::is_ok));
    assert_eq!(
        rig.harness.limiter.stats().get(LimitId::ShareTokenIp),
        Default::default()
    );
    rig.finish().await;
}
