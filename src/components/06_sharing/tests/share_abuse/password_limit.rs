//! AZ-3701 AC-2 to AC-4 (IT-06-8 wrong passwords, ST-06-6, NFT-SEC-06): `share_wrong_password_limit` wrong
//! passwords per link per `share_wrong_password_window` (500 ms in the harness, moved on the manual clock), counted
//! per link whatever the address; a limited link answers `RateLimited` even for the right password with no Argon2
//! check (G-06-2), and parallel attempts from many addresses run at most the limit of checks.

use std::sync::Arc;
use std::time::Duration;

use contracts::PublicShareError;
use sc_ids::RawToken;
use sc_ratelimit::LimitId;
use tokio::sync::Barrier;
use tokio::task::JoinSet;

use crate::rig::{IP1, IP2, Rig, address};
use crate::visitor_support::{PASSWORD, presented};

const WRONG: &str = "a wrong guess";

#[tokio::test]
async fn wrong_passwords_are_limited_per_link_whatever_the_address() {
    // Arrange
    let rig = Rig::new().await;
    let first = rig.link(Some(PASSWORD)).await;
    let second = rig.link(Some(PASSWORD)).await;
    let limit = rig.harness.config.wrong_password_limit;
    assert_eq!(limit, 10, "the AC-20 number");

    // Act
    let mut wrong = Vec::new();
    for _ in 0..limit {
        wrong.push(rig.shares.verify_password(&first.token, WRONG, IP1).await);
    }
    let eleventh = rig.shares.verify_password(&first.token, WRONG, IP1).await;
    let other_address = rig.shares.verify_password(&first.token, WRONG, IP2).await;
    let other_link = rig.shares.verify_password(&second.token, WRONG, IP1).await;

    // Assert
    assert!(
        wrong
            .iter()
            .all(|answer| matches!(answer, Err(PublicShareError::WrongPassword)))
    );
    for limited in [&eleventh, &other_address] {
        let Err(PublicShareError::RateLimited { retry_after }) = limited else {
            panic!("the link is limited: {limited:?}");
        };
        assert!(*retry_after > Duration::ZERO);
        assert!(*retry_after <= rig.harness.config.wrong_password_window);
    }
    assert!(matches!(other_link, Err(PublicShareError::WrongPassword)));
    assert_eq!(rig.shares.password_checks_run(), limit + 1);
    let counts = rig.harness.limiter.stats().get(LimitId::SharePasswordLink);
    assert_eq!((counts.tripped, counts.limited), (1, 2));
    assert_eq!(
        rig.harness.limiter.stats().get(LimitId::ShareTokenIp),
        Default::default(),
        "a wrong password is not an invalid token"
    );
    rig.finish().await;
}

#[tokio::test]
async fn the_right_password_on_a_limited_link_is_rate_limited_before_any_check_and_granted_after_the_window()
 {
    // Arrange
    let rig = Rig::new().await;
    let link = rig.link(Some(PASSWORD)).await;
    for _ in 0..rig.harness.config.wrong_password_limit {
        let answer = rig.shares.verify_password(&link.token, WRONG, IP1).await;
        assert!(matches!(answer, Err(PublicShareError::WrongPassword)));
    }
    let checks_before = rig.shares.password_checks_run();

    // Act
    let while_limited = rig.shares.verify_password(&link.token, PASSWORD, IP2).await;
    let checks_while_limited = rig.shares.password_checks_run() - checks_before;
    rig.harness
        .clock
        .tick(rig.harness.config.wrong_password_window);
    let after_window = rig.shares.verify_password(&link.token, PASSWORD, IP2).await;

    // Assert
    assert!(
        matches!(while_limited, Err(PublicShareError::RateLimited { .. })),
        "{while_limited:?}"
    );
    assert_eq!(
        checks_while_limited, 0,
        "the limit is checked before Argon2"
    );
    let grant = after_window.expect("the right password after the window");
    assert_eq!(grant.value.len(), 43);
    rig.finish().await;
}

#[tokio::test]
async fn a_right_password_forgets_the_wrong_ones_before_it() {
    // Arrange
    let rig = Rig::new().await;
    let link = rig.link(Some(PASSWORD)).await;
    let limit = rig.harness.config.wrong_password_limit;

    // Act
    for _ in 0..limit - 1 {
        let answer = rig.shares.verify_password(&link.token, WRONG, IP1).await;
        assert!(matches!(answer, Err(PublicShareError::WrongPassword)));
    }
    let granted = rig.shares.verify_password(&link.token, PASSWORD, IP2).await;
    let mut after = Vec::new();
    for _ in 0..limit {
        after.push(rig.shares.verify_password(&link.token, WRONG, IP1).await);
    }
    let over = rig.shares.verify_password(&link.token, WRONG, IP1).await;

    // Assert
    assert!(granted.is_ok(), "{granted:?}");
    assert!(
        after
            .iter()
            .all(|answer| matches!(answer, Err(PublicShareError::WrongPassword)))
    );
    assert!(matches!(over, Err(PublicShareError::RateLimited { .. })));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_parallel_attempts_from_fifty_addresses_run_at_most_the_limit_of_checks() {
    // Arrange
    let rig = Rig::new().await;
    let link = rig.link(Some(PASSWORD)).await;
    let shares = Arc::new(rig.harness.shares());
    let barrier = Arc::new(Barrier::new(50));
    let text = link.token.expose().to_owned();
    let mut callers = JoinSet::new();
    for n in 0..50u8 {
        // Every caller holds the token as 11 hands it over after normalising the path: its own value of the text.
        let token: RawToken = presented(text.clone());
        let (shares, barrier) = (Arc::clone(&shares), Arc::clone(&barrier));
        callers.spawn(async move {
            barrier.wait().await;
            shares
                .verify_password(&token, &format!("guess {n}"), address(n + 1))
                .await
        });
    }

    // Act
    let answers = callers.join_all().await;

    // Assert
    let limit = usize::try_from(rig.harness.config.wrong_password_limit).expect("fits");
    let wrong = answers
        .iter()
        .filter(|answer| matches!(answer, Err(PublicShareError::WrongPassword)))
        .count();
    let limited = answers
        .iter()
        .filter(|answer| matches!(answer, Err(PublicShareError::RateLimited { .. })))
        .count();
    assert_eq!((wrong, limited), (limit, 50 - limit), "{answers:?}");
    assert_eq!(
        shares.password_checks_run(),
        rig.harness.config.wrong_password_limit,
        "at most the limit of Argon2 checks"
    );
    drop(shares);
    rig.finish().await;
}
