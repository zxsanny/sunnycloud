//! IT-06-3 password part, NFT-SEC-06 and ST-06-5 (AZ-3699 AC-1, AC-2, AC-5, AC-7): `verify_password` gives a
//! grant expiring `share_grant_ttl` from now for the right password and `WrongPassword` for another; a token that
//! is not a live password link answers `NotFound` without a permit or a hash; at most
//! `share_password_max_concurrency` Argon2id checks run at once; stored passwords are Argon2id PHC strings with
//! the configured cost and a salt of their own. Links are made by the real `create` over the real tree.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use contracts::{CreatedShare, PublicShareError, ShareService};
use sc_clock::Clock;
use sc_config::Secret;
use sunnycloud_sharing::{GRANT_KEY_KEY, PgShareService};
use tokio::task::JoinSet;

use crate::content_support::Owner;
use crate::harness::{Harness, TEST_PHC_PREFIX};
use crate::owner_support::password_hash_of;
use crate::visitor_support::{HOUR, PASSWORD, VISITOR, malformed_tokens, presented, unknown_token};

const UNICODE_PASSWORD: &str = "пароль-🔑-é-密码";
/// An Argon2id cost that makes one check take milliseconds, so the 50 checks overlap.
const SLOW_ARGON2: &str = "8192,2,1";
/// A cost that keeps the one permit held while every not-found token is tried.
const HOLDING_ARGON2: &str = "65536,4,1";
const NOT_FOUND_BUDGET: Duration = Duration::from_millis(2);

/// 1,024 bytes of four-byte characters.
fn kilobyte_password() -> String {
    "🔑".repeat(256)
}

async fn link(
    shares: &PgShareService,
    owner: &Owner,
    node: uuid::Uuid,
    password: Option<&str>,
) -> CreatedShare {
    shares
        .create(owner.account, node, None, password)
        .await
        .expect("create link")
}

/// Waits until `shares` holds `count` permits; panics after one second.
async fn wait_for_checks(shares: &PgShareService, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while shares.password_checks_in_progress() != count {
        assert!(Instant::now() < deadline, "no check took a permit");
        tokio::time::sleep(Duration::from_micros(200)).await;
    }
}

#[tokio::test]
async fn the_right_password_gives_a_grant_for_the_ttl_and_a_wrong_one_does_not() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let shares = harness.shares();
    let long = kilobyte_password();
    let long_wrong = format!("{}🗝", "🔑".repeat(255));
    let mut cases = Vec::new();
    for (right, wrong) in [
        (PASSWORD, "correct horse "),
        (UNICODE_PASSWORD, "пароль-🔑-e-密码"),
        (long.as_str(), long_wrong.as_str()),
    ] {
        cases.push((
            link(&shares, &owner, file.node, Some(right)).await,
            right,
            wrong,
        ));
    }

    // Act
    let mut answers = Vec::new();
    for (created, right, wrong) in &cases {
        let granted = shares.verify_password(&created.token, right, VISITOR).await;
        let refused = shares.verify_password(&created.token, wrong, VISITOR).await;
        let empty = shares.verify_password(&created.token, "", VISITOR).await;
        answers.push((granted, refused, empty));
    }

    // Assert
    assert_eq!((long.len(), long_wrong.len()), (1024, 1024));
    let expected_expiry = harness
        .clock
        .now_utc()
        .checked_add(harness.config.grant_ttl)
        .expect("in range");
    assert_eq!(harness.config.grant_ttl, Duration::from_millis(500));
    for (granted, refused, empty) in answers {
        let grant = granted.expect("the right password gives a grant");
        assert_eq!(grant.expires_at, expected_expiry);
        assert_eq!(grant.value.len(), 43);
        assert!(!format!("{grant:?}").contains(&grant.value));
        assert!(matches!(refused, Err(PublicShareError::WrongPassword)));
        assert!(matches!(empty, Err(PublicShareError::WrongPassword)));
    }
    assert_eq!(shares.password_checks_in_progress(), 0);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn a_token_that_is_not_a_live_password_link_is_not_found_at_once_without_a_permit() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let mut config = harness.config.clone();
    config.password_max_concurrency = 1;
    config.argon2_params = Some(HOLDING_ARGON2.to_owned());
    let shares = Arc::new(harness.shares_with(&config));
    let create = async |ttl: Duration, password: Option<&str>| {
        shares
            .create(owner.account, file.node, Some(ttl), password)
            .await
            .expect("create link")
    };
    let slow = create(24 * HOUR, Some(PASSWORD)).await;
    let plain = create(24 * HOUR, None).await;
    let revoked = create(24 * HOUR, Some(PASSWORD)).await;
    let expiring = create(HOUR, Some(PASSWORD)).await;
    shares
        .revoke(owner.account, revoked.id)
        .await
        .expect("revoke");
    harness.clock.tick(HOUR);
    let mut tokens = vec![unknown_token(), plain.token, revoked.token, expiring.token];
    tokens.extend(malformed_tokens());
    // The only permit is held by a slow check of a live password link while the other tokens are tried.
    let holder = Arc::clone(&shares);
    let held = tokio::spawn(async move {
        holder
            .verify_password(&slow.token, "a wrong guess", VISITOR)
            .await
    });
    wait_for_checks(&shares, 1).await;

    // Act
    let mut answers = Vec::new();
    for token in &tokens {
        let mut elapsed = Vec::new();
        let mut last = None;
        for _ in 0..9 {
            let started = Instant::now();
            last = Some(shares.verify_password(token, PASSWORD, VISITOR).await);
            elapsed.push(started.elapsed());
        }
        elapsed.sort();
        answers.push((last.expect("answered"), elapsed[elapsed.len() / 2]));
    }
    let in_progress = shares.password_checks_in_progress();
    let still_held = !held.is_finished();
    let held = held.await.expect("the slow check finished");

    // Assert
    assert_eq!(answers.len(), 7);
    for (answer, median) in answers {
        assert!(
            matches!(answer, Err(PublicShareError::NotFound)),
            "{answer:?}"
        );
        assert!(median < NOT_FOUND_BUDGET, "median {median:?}");
    }
    assert!(still_held, "the slow check outlasted the not-found calls");
    assert_eq!(in_progress, 1, "only the slow check holds a permit");
    assert!(matches!(held, Err(PublicShareError::WrongPassword)));
    assert_eq!(shares.password_checks_in_progress(), 0);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn at_most_the_configured_number_of_checks_run_at_once() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let mut config = harness.config.clone();
    config.password_max_concurrency = 4;
    config.argon2_params = Some(SLOW_ARGON2.to_owned());
    // Room for all 50 wrong guesses on the one link: this test is about the permit bound, not the AC-20 limit.
    config.wrong_password_limit = 50;
    let shares = Arc::new(harness.shares_limited_by(&config));
    let created = link(&shares, &owner, file.node, Some(PASSWORD)).await;
    let mut callers = JoinSet::new();

    // Act
    for guess in 0..50 {
        let (shares, token) = (
            Arc::clone(&shares),
            presented(created.token.expose().to_owned()),
        );
        callers.spawn(async move {
            shares
                .verify_password(&token, &format!("guess {guess}"), VISITOR)
                .await
        });
    }
    let (mut peak, mut answers) = (0, Vec::new());
    while !callers.is_empty() {
        peak = peak.max(shares.password_checks_in_progress());
        match callers.try_join_next() {
            Some(joined) => answers.push(joined.expect("caller finished")),
            None => tokio::time::sleep(Duration::from_micros(100)).await,
        }
    }

    // Assert
    assert_eq!(answers.len(), 50);
    assert!(
        answers
            .iter()
            .all(|answer| matches!(answer, Err(PublicShareError::WrongPassword)))
    );
    assert_eq!(peak, 4, "the bound is reached and never passed");
    assert_eq!(shares.password_checks_in_progress(), 0);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn stored_passwords_are_argon2id_with_the_configured_cost_and_unique_salts() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let shares = harness.shares();
    let mut ids = Vec::new();
    for _ in 0..100 {
        ids.push(link(&shares, &owner, file.node, Some(PASSWORD)).await.id);
    }

    // Act
    let mut stored = Vec::new();
    for id in &ids {
        stored.push(
            password_hash_of(owner.probe(), *id)
                .await
                .expect("a password"),
        );
    }

    // Assert
    let mut salts = HashSet::new();
    for phc in &stored {
        assert!(phc.starts_with(TEST_PHC_PREFIX), "{phc}");
        let fields: Vec<&str> = phc.split('$').collect();
        assert_eq!(fields.len(), 6, "{phc}");
        assert_eq!(
            (fields[1], fields[2], fields[3]),
            ("argon2id", "v=19", "m=64,t=1,p=1")
        );
        salts.insert(fields[4].to_owned());
    }
    assert_eq!(salts.len(), 100, "no two links share a salt");
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn the_service_refuses_a_grant_key_too_short_to_sign_grants() {
    // Arrange
    let harness = Harness::new().await;
    let mut short = harness.config.clone();
    short.grant_key = Secret::new("k".repeat(31));
    let mut empty = harness.config.clone();
    empty.grant_key = Secret::new(String::new());

    // Act
    let refused = [
        harness.try_shares(&short, harness.audit_writer()),
        harness.try_shares(&empty, harness.audit_writer()),
    ];

    // Assert
    for outcome in refused {
        let problem = outcome.err().expect("refused");
        assert_eq!(problem.key, GRANT_KEY_KEY);
    }
    harness.finish().await;
}
