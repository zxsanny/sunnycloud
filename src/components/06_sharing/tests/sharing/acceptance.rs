//! AT-06-1, AT-06-2 and AT-06-3 (AZ-3704 AC-1 to AC-3): the owner-visible link scenarios at the sharing port
//! boundary, over files published, edited, renamed, moved and deleted through the real 02. AT-06-1's photo is a
//! DS-03-shaped revision (a 3 MiB JPEG name, size and sniffed `image/jpeg`): 06 reads only those columns, never the
//! bytes. There is no lookup cache (owner decision 2026-11-14), so "within the cache window" and "within 5 s" are
//! checked as the very next call, timed against the 5 s bound.

use std::time::{Duration, Instant};

use contracts::{PublicShareError, ShareOpen, ShareService};
use sc_clock::Clock;
use sunnycloud_sharing::ShareConfig;

use crate::content_support::{Body, Owner, content_of};
use crate::harness::Harness;
use crate::owner_support::{audit_rows, link_rows_as_text};
use crate::settings::{TEST_GRANT_KEY, load_settings};
use crate::visitor_support::{PASSWORD, VISITOR};

/// A DS-03 photo as 02 records it after the upload: 3 MiB, inside DS-03's 2.5-5 MB range.
const PHOTO: Body = Body {
    size: 3 * 1024 * 1024,
    media_type: "image/jpeg",
};
const PHOTO_NAME: &str = "IMG_0042.jpg";
const DAY: Duration = Duration::from_secs(86_400);
const WRONG: &str = "a wrong guess";
/// AC-77: a deleted file's link answers `NotFound` within this bound.
const DELETE_BOUND: Duration = Duration::from_secs(5);

async fn accessed_events(owner: &mut Owner) -> usize {
    audit_rows(owner.probe())
        .await
        .into_iter()
        .filter(|row| row.0 == "share_accessed")
        .count()
}

#[tokio::test]
async fn at_06_1_a_shared_photo_opens_downloads_once_and_is_not_found_after_revoke() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let photo = owner
        .new_file_of(&harness, owner.root, PHOTO_NAME, PHOTO)
        .await;
    let production = ShareConfig::from_settings(&load_settings(TEST_GRANT_KEY.as_bytes(), &[]));
    let mut config = harness.config.clone();
    config.default_ttl = production.default_ttl;
    let shares = harness.shares_with(&config);
    let created_at = harness.clock.now_utc();

    // Act
    let created = shares
        .create(owner.account, photo.node, None, None)
        .await
        .expect("create with the default expiry");
    let stored = link_rows_as_text(owner.probe()).await;
    let listed = shares
        .list(owner.account, None)
        .await
        .expect("list the owner's links");
    let opened = shares.open(&created.token, VISITOR).await;
    let content = shares
        .authorize_content(&created.token, None, VISITOR, true)
        .await;
    let events = accessed_events(&mut owner).await;
    shares
        .revoke(owner.account, created.id)
        .await
        .expect("revoke");
    let after_revoke = shares.open(&created.token, VISITOR).await;
    let content_after_revoke = shares
        .authorize_content(&created.token, None, VISITOR, true)
        .await;

    // Assert
    let expires_at = created_at.checked_add(7 * DAY).expect("in range");
    assert_eq!(production.default_ttl, 7 * DAY);
    assert_eq!(created.expires_at, expires_at);
    assert!(!created.has_password);
    assert_eq!(stored.len(), 1);
    assert!(
        !stored[0].contains(created.token.expose()),
        "the token is shown once: only its hash is stored"
    );
    assert_eq!(
        (listed.len(), listed[0].id, listed[0].expires_at),
        (1, created.id, expires_at)
    );
    assert_eq!(
        opened.expect("open"),
        ShareOpen::Available {
            name: PHOTO_NAME.to_owned(),
            size: PHOTO.size,
            media_type: Some("image/jpeg".to_owned()),
            expires_at,
        }
    );
    assert_eq!(content.expect("authorized"), content_of(&owner, photo));
    assert_eq!(events, 1, "one share_accessed event");
    assert_eq!(after_revoke.expect("open"), ShareOpen::NotFound);
    assert!(matches!(
        content_after_revoke,
        Err(PublicShareError::NotFound)
    ));
    assert_eq!(accessed_events(&mut owner).await, 1);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn at_06_2_a_password_link_limits_wrong_guesses_and_grants_content_after_the_window() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "tax-2026.txt").await;
    let shares = harness.shares();
    let link = shares
        .create(owner.account, file.node, None, Some(PASSWORD))
        .await
        .expect("create a password link");
    let limit = harness.config.wrong_password_limit;
    let window = harness.config.wrong_password_window;

    // Act
    let opened = shares.open(&link.token, VISITOR).await;
    let mut wrong = Vec::new();
    for _ in 0..limit {
        wrong.push(shares.verify_password(&link.token, WRONG, VISITOR).await);
    }
    let eleventh = shares.verify_password(&link.token, WRONG, VISITOR).await;
    harness.clock.tick(window);
    let granted = shares.verify_password(&link.token, PASSWORD, VISITOR).await;
    let grant = granted.expect("the right password after the window gives a grant");
    let content = shares
        .authorize_content(&link.token, Some(&grant), VISITOR, true)
        .await;

    // Assert
    assert_eq!(limit, 10, "the AC-20 number");
    assert_eq!(opened.expect("open"), ShareOpen::PasswordRequired);
    assert_eq!(wrong.len(), 10);
    assert!(
        wrong
            .iter()
            .all(|answer| matches!(answer, Err(PublicShareError::WrongPassword))),
        "{wrong:?}"
    );
    let Err(PublicShareError::RateLimited { retry_after }) = eleventh else {
        panic!("the 11th guess is limited: {eleventh:?}");
    };
    assert!(retry_after > Duration::ZERO && retry_after <= window);
    assert_eq!(content.expect("granted content"), content_of(&owner, file));
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn at_06_3_a_link_follows_edits_renames_and_moves_and_is_not_found_within_5_s_of_delete() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let archive = owner.folder(owner.root, "archive").await;
    let notes = owner.new_file(&harness, owner.root, "notes.txt").await;
    let shares = harness.shares();
    let link = shares
        .create(owner.account, notes.node, Some(DAY), None)
        .await
        .expect("create a link on notes.txt");
    let rows_before = link_rows_as_text(owner.probe()).await;

    // Act
    let (_, edited) = owner.edit(&harness, notes).await;
    let after_edit = shares
        .authorize_content(&link.token, None, VISITOR, false)
        .await;
    owner
        .update(notes.node, Some("notes-final.txt"), Some(archive))
        .await;
    let opened_after_move = shares.open(&link.token, VISITOR).await;
    let after_move = shares
        .authorize_content(&link.token, None, VISITOR, false)
        .await;
    let rows_after_move = link_rows_as_text(owner.probe()).await;
    owner.delete(notes.node).await;
    let deleted_at = Instant::now();
    let opened_after_delete = shares.open(&link.token, VISITOR).await;
    let after_delete = shares
        .authorize_content(&link.token, None, VISITOR, false)
        .await;
    let not_found_after = deleted_at.elapsed();

    // Assert
    assert_ne!(edited.revision, notes.revision);
    assert_eq!(
        after_edit.expect("after the edit"),
        content_of(&owner, edited)
    );
    let ShareOpen::Available { name, .. } = opened_after_move.expect("open") else {
        panic!("the link still works after rename and move");
    };
    assert_eq!(name, "notes-final.txt");
    assert_eq!(
        after_move.expect("after the move"),
        content_of(&owner, edited)
    );
    assert_eq!(rows_after_move, rows_before, "the same link, unchanged");
    assert_eq!(opened_after_delete.expect("open"), ShareOpen::NotFound);
    assert!(matches!(after_delete, Err(PublicShareError::NotFound)));
    assert!(
        not_found_after <= DELETE_BOUND,
        "NotFound {not_found_after:?} after the delete"
    );
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}
