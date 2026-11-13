//! IT-06-1 create (AZ-3697 AC-1 to AC-4): the default and the range of the expiry, the target rules over the real
//! tree, and what `share_links` holds at rest.

use std::time::Duration;

use contracts::{ShareError, ShareService};
use sc_ids::sha256_token;
use sqlx::Connection;
use sunnycloud_sharing::SHARE_TOKEN_PREFIX;
use uuid::Uuid;

use crate::harness::{Harness, START, TEST_PHC_PREFIX};
use crate::owner_support::{
    account_with_root, link_count, link_rows_as_text, live_file, node, password_hash_of, purge,
    token_hash_of, tombstone,
};

const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(86_400);
const PASSWORD: &str = "correct horse";

/// A harness and one live file of a fresh account.
async fn owner_with_file() -> (Harness, Uuid, Uuid) {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    let root = account_with_root(&mut conn, account).await;
    let file = live_file(&mut conn, account, root).await;
    conn.close().await.expect("close fixture connection");
    (harness, account, file)
}

#[tokio::test]
async fn create_without_expiry_or_password_gives_a_token_the_url_and_the_default_expiry() {
    // Arrange
    let (harness, account, file) = owner_with_file().await;
    let mut config = harness.config.clone();
    config.default_ttl = 7 * DAY;
    let shares = harness.shares_with(&config);

    // Act
    let created = shares
        .create(account, file, None, None)
        .await
        .expect("create");

    // Assert
    assert!(created.token.expose().starts_with(SHARE_TOKEN_PREFIX));
    assert_eq!(created.url_path, format!("/s/{}", created.token.expose()));
    assert_eq!(
        created.expires_at,
        START.checked_add(7 * DAY).expect("in range")
    );
    assert!(!created.has_password);
    assert!(!format!("{:?}", created.token).contains(created.token.expose()));
    drop(shares);
    harness.finish().await;
}

#[tokio::test]
async fn expiry_of_one_hour_and_365_days_is_accepted_and_366_days_and_zero_are_refused() {
    // Arrange
    let (harness, account, file) = owner_with_file().await;
    let shares = harness.shares();

    // Act
    let mut accepted = Vec::new();
    for ttl in [HOUR, 365 * DAY] {
        let created = shares
            .create(account, file, Some(ttl), None)
            .await
            .expect("accepted expiry");
        accepted.push((ttl, created.expires_at));
    }
    let mut refused = Vec::new();
    for ttl in [366 * DAY, Duration::ZERO] {
        refused.push(shares.create(account, file, Some(ttl), None).await);
    }

    // Assert
    for (ttl, expires_at) in accepted {
        assert_eq!(expires_at, START.checked_add(ttl).expect("in range"));
    }
    for outcome in refused {
        assert!(
            matches!(outcome, Err(ShareError::ExpiryOutOfRange)),
            "{outcome:?}"
        );
    }
    drop(shares);
    harness.finish().await;
}

#[tokio::test]
async fn a_folder_is_not_shareable_and_every_gone_target_is_the_same_not_found() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let root_of_a = account_with_root(&mut conn, a).await;
    let root_of_b = account_with_root(&mut conn, b).await;
    let folder = node(&mut conn, a, Some(root_of_a), "folder", "albums").await;
    let tombstoned = live_file(&mut conn, a, root_of_a).await;
    tombstone(&mut conn, tombstoned).await;
    let purged = live_file(&mut conn, a, root_of_a).await;
    purge(&mut conn, purged).await;
    let file_of_b = live_file(&mut conn, b, root_of_b).await;
    let shares = harness.shares();

    // Act
    let on_folder = shares.create(a, folder, None, None).await;
    let mut gone = Vec::new();
    for target in [tombstoned, purged, file_of_b, Uuid::new_v4()] {
        let outcome = shares.create(a, target, None, Some(PASSWORD)).await;
        gone.push(format!("{outcome:?}"));
    }
    let links = link_count(&mut conn).await;

    // Assert
    assert!(
        matches!(on_folder, Err(ShareError::TargetNotShareable)),
        "{on_folder:?}"
    );
    let expected = format!("{:?}", Err::<(), _>(ShareError::TargetNotFound));
    assert_eq!(gone, vec![expected; 4]);
    assert_eq!(links, 0);
    conn.close().await.expect("close fixture connection");
    drop(shares);
    harness.finish().await;
}

#[tokio::test]
async fn only_the_token_hash_and_an_argon2id_phc_string_are_stored() {
    // Arrange
    let (harness, account, file) = owner_with_file().await;
    let shares = harness.shares();

    // Act
    let created = shares
        .create(account, file, None, Some(PASSWORD))
        .await
        .expect("create");
    let mut conn = harness.fixture_conn().await;
    let stored_hash = token_hash_of(&mut conn, created.id).await;
    let password_hash = password_hash_of(&mut conn, created.id).await;
    let rows = link_rows_as_text(&mut conn).await;

    // Assert
    assert!(created.has_password);
    assert_eq!(
        stored_hash,
        sha256_token(created.token.expose()).as_bytes().to_vec()
    );
    let password_hash = password_hash.expect("a password hash");
    assert!(
        password_hash.starts_with(TEST_PHC_PREFIX),
        "{password_hash}"
    );
    let body = created
        .token
        .expose()
        .strip_prefix(SHARE_TOKEN_PREFIX)
        .expect("prefix");
    for row in &rows {
        assert!(!row.contains(body), "token in {row}");
        assert!(!row.contains(PASSWORD), "password in {row}");
    }
    conn.close().await.expect("close fixture connection");
    drop(shares);
    harness.finish().await;
}

#[tokio::test]
async fn two_links_with_the_same_password_have_different_salts() {
    // Arrange
    let (harness, account, file) = owner_with_file().await;
    let shares = harness.shares();

    // Act
    let first = shares
        .create(account, file, None, Some(PASSWORD))
        .await
        .expect("first");
    let second = shares
        .create(account, file, None, Some(PASSWORD))
        .await
        .expect("second");
    let mut conn = harness.fixture_conn().await;
    let first_hash = password_hash_of(&mut conn, first.id).await;
    let second_hash = password_hash_of(&mut conn, second.id).await;

    // Assert
    assert_ne!(first_hash, second_hash);
    assert_ne!(first.token.expose(), second.token.expose());
    conn.close().await.expect("close fixture connection");
    drop(shares);
    harness.finish().await;
}
