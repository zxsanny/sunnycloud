//! IT-06-2 open outcomes (AZ-3698 AC-1 to AC-3, AC-6): a live link shows its file's current revision, a password
//! link shows nothing before unlock, and every dead case, the expiry boundary and every malformed token give the
//! one `NotFound`.

use contracts::{ShareOpen, ShareService};
use sqlx::Connection;
use sunnycloud_sharing::PgShareService;
use uuid::Uuid;

use crate::harness::Harness;
use crate::owner_support::account_with_root;
use crate::visitor_support::{
    HOUR, VISITOR, cases, malformed_tokens, named_file, new_current_revision, presented,
    unknown_token,
};

#[tokio::test]
async fn a_live_link_shows_the_current_revision_before_and_after_a_save_in_place() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    let root = account_with_root(&mut conn, account).await;
    let file = named_file(&mut conn, account, root, "photo.jpg", 1000, "image/jpeg").await;
    let shares = harness.shares();
    let link = shares
        .create(account, file, Some(HOUR), None)
        .await
        .expect("create");

    // Act
    let before = shares.open(&link.token, VISITOR).await.expect("open");
    new_current_revision(&mut conn, account, file, 2048, "image/png").await;
    let after = shares.open(&link.token, VISITOR).await.expect("open");

    // Assert
    let expected = |size, media_type: &str| ShareOpen::Available {
        name: "photo.jpg".to_owned(),
        size,
        media_type: Some(media_type.to_owned()),
        expires_at: link.expires_at,
    };
    assert_eq!(before, expected(1000, "image/jpeg"));
    assert_eq!(after, expected(2048, "image/png"));
    conn.close().await.expect("close fixture connection");
    drop(shares);
    harness.finish().await;
}

#[tokio::test]
async fn a_live_password_link_answers_password_required_with_no_file_data() {
    // Arrange
    let harness = Harness::new().await;
    let cases = cases(&harness).await;

    // Act
    let answer = cases
        .shares
        .open(&cases.password, VISITOR)
        .await
        .expect("open");

    // Assert
    assert_eq!(answer, ShareOpen::PasswordRequired);
    assert_eq!(format!("{answer:?}"), "PasswordRequired");
    drop(cases);
    harness.finish().await;
}

#[tokio::test]
async fn every_dead_case_and_every_malformed_token_is_the_same_not_found() {
    // Arrange
    let harness = Harness::new().await;
    let cases = cases(&harness).await;
    let (unknown, malformed) = (unknown_token(), malformed_tokens());
    let mut tokens = vec![
        &cases.expired,
        &cases.expired_password,
        &cases.revoked,
        &cases.tombstoned,
        &cases.purged,
        &unknown,
    ];
    tokens.extend(&malformed);

    // Act
    let mut answers = Vec::new();
    for token in tokens {
        answers.push(cases.shares.open(token, VISITOR).await.expect("open"));
    }

    // Assert
    assert_eq!(answers.len(), 9);
    for answer in &answers {
        assert_eq!(*answer, ShareOpen::NotFound);
        assert_eq!(format!("{answer:?}"), "NotFound");
    }
    drop(cases);
    harness.finish().await;
}

#[tokio::test]
async fn a_link_is_live_one_microsecond_before_its_expiry_and_gone_at_it() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    let root = account_with_root(&mut conn, account).await;
    let file = named_file(&mut conn, account, root, "edge.txt", 3, "text/plain").await;
    conn.close().await.expect("close fixture connection");
    let shares = harness.shares();
    let link = shares
        .create(account, file, Some(HOUR), None)
        .await
        .expect("create");
    let micro = std::time::Duration::from_micros(1);

    // Act
    harness.clock.tick(HOUR - micro);
    let before = shares.open(&link.token, VISITOR).await.expect("open");
    harness.clock.tick(micro);
    let at = shares.open(&link.token, VISITOR).await.expect("open");

    // Assert
    assert!(matches!(before, ShareOpen::Available { .. }), "{before:?}");
    assert_eq!(at, ShareOpen::NotFound);
    drop(shares);
    harness.finish().await;
}

#[tokio::test]
async fn the_lookup_matches_the_exact_token_only_never_a_part_of_it() {
    // Arrange
    let harness = Harness::new().await;
    let cases = cases(&harness).await;
    let full = cases.live.expose().to_owned();
    let partial = [
        full[..full.len() - 1].to_owned(),
        format!("{full}A"),
        full.to_uppercase(),
    ];

    // Act
    let exact = open(&cases.shares, full).await;
    let mut others = Vec::new();
    for text in partial {
        others.push(open(&cases.shares, text).await);
    }

    // Assert
    assert!(matches!(exact, ShareOpen::Available { .. }), "{exact:?}");
    assert_eq!(others.len(), 3);
    assert!(
        others.iter().all(|answer| *answer == ShareOpen::NotFound),
        "{others:?}"
    );
    drop(cases);
    harness.finish().await;
}

#[tokio::test]
async fn a_revoked_link_stays_not_found_while_other_links_on_its_file_work() {
    // Arrange
    let harness = Harness::new().await;
    let cases = cases(&harness).await;
    let fresh = cases
        .shares
        .create(cases.account, cases.live_file, None, None)
        .await
        .expect("another link on the same file");

    // Act
    let revoked = cases.shares.open(&cases.revoked, VISITOR).await;
    let other = cases.shares.open(&fresh.token, VISITOR).await;

    // Assert
    assert_eq!(revoked.expect("open"), ShareOpen::NotFound);
    assert!(matches!(other.expect("open"), ShareOpen::Available { .. }));
    drop(cases);
    harness.finish().await;
}

async fn open(shares: &PgShareService, text: String) -> ShareOpen {
    shares.open(&presented(text), VISITOR).await.expect("open")
}
