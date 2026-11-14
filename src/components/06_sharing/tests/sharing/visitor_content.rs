//! IT-06-4, IT-06-5 and IT-06-6 (AZ-3700 AC-1 to AC-4, AC-77): `authorize_content` re-reads the link and its file on
//! every call (no cache, owner decision 2026-11-14), so it returns the current revision after a save in place,
//! follows renames and moves by node id, answers `NotFound` on the very next call after a delete (also before 02's
//! cascade has marked a deleted folder's files), works again after an edit revives the file and is `NotFound`
//! once the purge has removed the row. The file changes go through the real 02.

use contracts::{
    AccessGrant, CreatedShare, PublicShareError, PublishOutcome, ShareContent, ShareOpen,
    ShareService,
};
use sc_clock::Clock;
use sc_ids::RawToken;
use sunnycloud_sharing::PgShareService;

use crate::content_support::{Owner, Version, content_of};
use crate::harness::Harness;
use crate::owner_support::{audit_rows, link_count, link_rows_as_text};
use crate::visitor_support::{HOUR, PASSWORD, VISITOR};

async fn authorize(
    shares: &PgShareService,
    token: &RawToken,
    starts_download: bool,
) -> Result<ShareContent, PublicShareError> {
    shares
        .authorize_content(token, None, VISITOR, starts_download)
        .await
}

async fn share(shares: &PgShareService, owner: &Owner, file: Version) -> CreatedShare {
    shares
        .create(owner.account, file.node, Some(24 * HOUR), None)
        .await
        .expect("create link")
}

/// The name `open` shows for a live plain link.
async fn shown_name(shares: &PgShareService, token: &RawToken) -> String {
    match shares.open(token, VISITOR).await.expect("open") {
        ShareOpen::Available { name, .. } => name,
        other => panic!("expected Available, got {other:?}"),
    }
}

#[tokio::test]
async fn the_current_revision_is_returned_before_and_right_after_a_save_in_place() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let r1 = owner.new_file(&harness, owner.root, "f.txt").await;
    let shares = harness.shares();
    let link = share(&shares, &owner, r1).await;
    let rows_before = link_rows_as_text(owner.probe()).await;

    // Act
    let first = authorize(&shares, &link.token, false).await;
    let (outcome, r2) = owner.edit(&harness, r1).await;
    let second = authorize(&shares, &link.token, false).await;

    // Assert
    assert_eq!(outcome, PublishOutcome::Updated);
    assert_eq!((r2.node, r2.revision == r1.revision), (r1.node, false));
    assert_eq!(first.expect("first"), content_of(&owner, r1));
    assert_eq!(second.expect("after the edit"), content_of(&owner, r2));
    assert_eq!(link_rows_as_text(owner.probe()).await, rows_before);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn only_calls_that_start_a_download_write_one_access_event_with_ids_only() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "report-q3.txt").await;
    let shares = harness.shares();
    let link = share(&shares, &owner, file).await;

    // Act
    for starts_download in [
        true, false, true, false, true, false, true, false, true, false,
    ] {
        authorize(&shares, &link.token, starts_download)
            .await
            .expect("allowed");
    }

    // Assert
    let accessed: Vec<_> = audit_rows(owner.probe())
        .await
        .into_iter()
        .filter(|row| row.0 == "share_accessed")
        .collect();
    assert_eq!(accessed.len(), 5);
    for (_, actor, subject_type, subject_id, detail) in &accessed {
        let detail = detail.as_deref().expect("detail");
        assert_eq!(
            (*actor, subject_type.as_deref(), *subject_id),
            (None, Some("share_link"), Some(link.id))
        );
        for id in [owner.account, file.node, file.revision] {
            assert!(detail.contains(&id.to_string()), "{detail}");
        }
        assert!(!detail.contains(link.token.expose()) && !detail.contains("report-q3"));
    }
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn a_password_link_needs_a_grant_and_a_plain_link_is_allowed_without_one() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let shares = harness.shares();
    let plain = share(&shares, &owner, file).await;
    let protected = shares
        .create(owner.account, file.node, None, Some(PASSWORD))
        .await
        .expect("password link");
    // A value no grant key produced counts as absent (valid grants: `visitor_grant`).
    let presented = AccessGrant {
        value: "not-a-grant-of-this-link".to_owned(),
        expires_at: harness.clock.now_utc(),
    };

    // Act
    let without_grant = authorize(&shares, &protected.token, true).await;
    let with_grant = shares
        .authorize_content(&protected.token, Some(&presented), VISITOR, true)
        .await;
    let allowed = authorize(&shares, &plain.token, true).await;

    // Assert
    assert!(matches!(
        without_grant,
        Err(PublicShareError::PasswordRequired)
    ));
    assert!(matches!(
        with_grant,
        Err(PublicShareError::PasswordRequired)
    ));
    assert_eq!(allowed.expect("plain link"), content_of(&owner, file));
    let accessed = audit_rows(owner.probe())
        .await
        .into_iter()
        .filter(|row| row.0 == "share_accessed")
        .count();
    assert_eq!(accessed, 1, "a refused call is not an access");
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn the_link_follows_renames_and_moves_and_is_not_found_right_after_the_delete() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let a = owner.folder(owner.root, "a").await;
    let b = owner.folder(owner.root, "b").await;
    let deep = owner.folder(b, "c").await;
    let deeper = owner.folder(deep, "d").await;
    let file = owner.new_file(&harness, a, "f.txt").await;
    let shares = harness.shares();
    let link = share(&shares, &owner, file).await;
    let rows_before = link_rows_as_text(owner.probe()).await;
    let steps: [(Option<&str>, Option<_>, &str); 4] = [
        (Some("g.txt"), None, "g.txt"),
        (None, Some(b), "g.txt"),
        (Some("G.TXT"), None, "G.TXT"),
        (None, Some(deeper), "G.TXT"),
    ];

    // Act
    let mut seen = Vec::new();
    for (name, parent, _) in steps {
        owner.update(file.node, name, parent).await;
        let content = authorize(&shares, &link.token, false).await;
        seen.push((shown_name(&shares, &link.token).await, content));
    }
    let rows_after_moves = link_rows_as_text(owner.probe()).await;
    owner.delete(file.node).await;
    let after_delete = authorize(&shares, &link.token, false).await;
    let open_after_delete = shares.open(&link.token, VISITOR).await;

    // Assert
    for ((_, _, expected_name), (name, content)) in steps.iter().zip(seen) {
        assert_eq!(name, *expected_name);
        assert_eq!(content.expect("live"), content_of(&owner, file));
    }
    assert_eq!(rows_after_moves, rows_before, "0 changes to share_links");
    assert!(matches!(after_delete, Err(PublicShareError::NotFound)));
    assert_eq!(open_after_delete.expect("open"), ShareOpen::NotFound);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn a_file_under_a_deleted_folder_is_not_found_before_the_cascade_marks_it() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let folder = owner.folder(owner.root, "holder").await;
    let file = owner.new_file(&harness, folder, "inside.txt").await;
    let shares = harness.shares();
    let link = share(&shares, &owner, file).await;
    let before = authorize(&shares, &link.token, false).await;

    // Act
    owner.delete(folder).await;
    let after = authorize(&shares, &link.token, false).await;
    let open_after = shares.open(&link.token, VISITOR).await;

    // Assert
    assert_eq!(before.expect("live"), content_of(&owner, file));
    assert!(!owner.is_marked(file.node).await, "the cascade has not run");
    assert!(matches!(after, Err(PublicShareError::NotFound)));
    assert_eq!(open_after.expect("open"), ShareOpen::NotFound);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn an_edit_revives_the_link_with_the_same_token_and_the_purge_removes_its_row() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let shares = harness.shares();
    let link = share(&shares, &owner, file).await;
    owner.delete(file.node).await;
    let while_deleted = authorize(&shares, &link.token, false).await;

    // Act
    let (outcome, revived) = owner.edit(&harness, file).await;
    let after_revival = authorize(&shares, &link.token, false).await;
    owner.delete(file.node).await;
    owner.purge_past_horizon(&harness).await;
    let after_purge = authorize(&shares, &link.token, false).await;

    // Assert
    assert!(matches!(while_deleted, Err(PublicShareError::NotFound)));
    assert_eq!(outcome, PublishOutcome::Revived);
    assert_eq!(after_revival.expect("revived"), content_of(&owner, revived));
    assert_eq!(
        link_count(owner.probe()).await,
        0,
        "the row went with the node"
    );
    assert!(matches!(after_purge, Err(PublicShareError::NotFound)));
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}
