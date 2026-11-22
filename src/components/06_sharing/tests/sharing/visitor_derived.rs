//! AZ-4310 AC-1, AC-2 and AC-4: `authorize_derived` (a share thumbnail or preview) answers with the owning account
//! and the live file exactly as 02's `resolve_live_file` reads it, under the same lookup, liveness and grant rules as
//! `authorize_content`, and writes no `share_accessed` event; the four visitor operations answer through
//! `dyn PublicShareService`. The file changes go through the real 02.

use std::sync::Arc;

use contracts::{
    CreatedShare, LiveFile, LiveFileLookup, PublicShareError, PublicShareService, ShareDerived,
    ShareOpen, ShareService,
};
use sc_ids::{RawToken, sha256_token};
use sc_testdb::START;
use sunnycloud_sharing::{PgShareService, SHARE_TOKEN_PREFIX};
use sunnycloud_tree::PgTreeReader;
use uuid::Uuid;

use crate::content_support::{Body, Owner, Version, content_of};
use crate::harness::Harness;
use crate::owner_support::audit_rows;
use crate::visitor_support::{HOUR, PASSWORD, VISITOR, cases, malformed_tokens, presented};

const JPEG: Body = Body {
    size: 2048,
    media_type: "image/jpeg",
};

async fn share(
    shares: &PgShareService,
    owner: &Owner,
    file: Version,
    password: Option<&str>,
) -> CreatedShare {
    shares
        .create(owner.account, file.node, Some(24 * HOUR), password)
        .await
        .expect("create link")
}

/// The live file as 02's real reader resolves it for `owner`.
async fn resolved(harness: &Harness, owner: &Owner, file: Version) -> LiveFile {
    let reader = PgTreeReader::new(harness.api.clone(), &harness.tree_config);
    match reader.resolve_live_file(owner.account, file.node).await {
        Ok(LiveFileLookup::Found(found)) => found,
        other => panic!("the shared file is live: {other:?}"),
    }
}

/// A link row on `owner`'s folder, written as the schema owner: `create` refuses a folder, so only a row made
/// outside it can point at one. Returns the token whose hash the row holds.
async fn folder_link(owner: &mut Owner, folder: Uuid) -> RawToken {
    let token = presented(format!("{SHARE_TOKEN_PREFIX}{}", "F".repeat(43)));
    let expires_at = START.as_unix_micros() + 1_000_000 * 24 * 3600;
    let account = owner.account;
    sqlx::query(
        "INSERT INTO share_links (id, account_id, node_id, token_hash, expires_at, created_at) \
         VALUES ($1, $2, $3, $4, 'epoch'::timestamptz + $5::bigint * interval '1 microsecond', \
         'epoch'::timestamptz)",
    )
    .bind(Uuid::new_v4())
    .bind(account)
    .bind(folder)
    .bind(sha256_token(token.expose()).as_bytes().as_slice())
    .bind(expires_at)
    .execute(owner.probe())
    .await
    .expect("insert folder link");
    token
}

async fn accessed_events(owner: &mut Owner) -> usize {
    audit_rows(owner.probe())
        .await
        .into_iter()
        .filter(|row| row.0 == "share_accessed")
        .count()
}

#[tokio::test]
async fn a_live_link_on_a_jpeg_answers_the_owning_account_and_the_live_file_without_an_access_event()
 {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let photo = owner
        .new_file_of(&harness, owner.root, "photo.jpg", JPEG)
        .await;
    let shares = harness.shares();
    let link = share(&shares, &owner, photo, None).await;
    let expected = resolved(&harness, &owner, photo).await;

    // Act
    let derived = shares.authorize_derived(&link.token, None, VISITOR).await;

    // Assert
    let derived = derived.expect("allowed");
    assert_eq!(
        derived,
        ShareDerived {
            account_id: owner.account,
            file: expected,
        }
    );
    let revision = derived.file.revision.expect("current revision");
    assert_eq!(
        (revision.id, revision.media_type.as_deref()),
        (photo.revision, Some("image/jpeg"))
    );
    assert_eq!(
        accessed_events(&mut owner).await,
        0,
        "a thumbnail is not a download"
    );
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn every_dead_link_a_deleted_file_and_a_folder_answer_one_not_found() {
    // Arrange
    let harness = Harness::new().await;
    let cases = cases(&harness).await;
    let mut owner = Owner::new(&harness).await;
    let photo = owner
        .new_file_of(&harness, owner.root, "photo.jpg", JPEG)
        .await;
    let deleted = share(&cases.shares, &owner, photo, None).await;
    let folder = owner.folder(owner.root, "album").await;
    let on_folder = folder_link(&mut owner, folder).await;
    owner.delete(photo.node).await;
    let [empty, long, non_ascii] = malformed_tokens();
    let unknown = presented(format!("{SHARE_TOKEN_PREFIX}{}", "A".repeat(43)));
    let dead = [
        &unknown,
        &empty,
        &long,
        &non_ascii,
        &cases.expired,
        &cases.expired_password,
        &cases.revoked,
        &cases.tombstoned,
        &cases.purged,
        &deleted.token,
        &on_folder,
    ];

    // Act
    let mut answers = Vec::new();
    for token in dead {
        answers.push(cases.shares.authorize_derived(token, None, VISITOR).await);
    }
    let live = cases
        .shares
        .authorize_derived(&cases.live, None, VISITOR)
        .await;

    // Assert
    assert_eq!(answers.len(), 11);
    for answer in &answers {
        assert!(
            matches!(answer, Err(PublicShareError::NotFound)),
            "{answer:?}"
        );
    }
    assert_eq!(live.expect("live link").account_id, cases.account);
    drop(cases);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn a_password_link_answers_password_required_without_its_grant_and_the_file_with_it() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let photo = owner
        .new_file_of(&harness, owner.root, "photo.jpg", JPEG)
        .await;
    let shares = harness.shares();
    let protected = share(&shares, &owner, photo, Some(PASSWORD)).await;
    let other = share(&shares, &owner, photo, Some(PASSWORD)).await;
    let expected = resolved(&harness, &owner, photo).await;
    let other_grant = shares
        .verify_password(&other.token, PASSWORD, VISITOR)
        .await
        .expect("grant of the other link");

    // Act
    let without_grant = shares
        .authorize_derived(&protected.token, None, VISITOR)
        .await;
    let with_other_grant = shares
        .authorize_derived(&protected.token, Some(&other_grant), VISITOR)
        .await;
    let grant = shares
        .verify_password(&protected.token, PASSWORD, VISITOR)
        .await
        .expect("grant");
    let with_grant = shares
        .authorize_derived(&protected.token, Some(&grant), VISITOR)
        .await;

    // Assert
    assert!(matches!(
        without_grant,
        Err(PublicShareError::PasswordRequired)
    ));
    assert!(matches!(
        with_other_grant,
        Err(PublicShareError::PasswordRequired)
    ));
    assert_eq!(
        with_grant.expect("granted"),
        ShareDerived {
            account_id: owner.account,
            file: expected,
        }
    );
    assert_eq!(accessed_events(&mut owner).await, 0);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn the_four_visitor_operations_answer_through_the_port() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let photo = owner
        .new_file_of(&harness, owner.root, "photo.jpg", JPEG)
        .await;
    let shares = Arc::new(harness.shares());
    let plain = share(&shares, &owner, photo, None).await;
    let protected = share(&shares, &owner, photo, Some(PASSWORD)).await;
    let expected = resolved(&harness, &owner, photo).await;
    let port: Arc<dyn PublicShareService> = shares.clone();

    // Act
    let opened = port.open(&plain.token, None, VISITOR).await;
    let grant = port
        .verify_password(&protected.token, PASSWORD, VISITOR)
        .await;
    let grant = grant.expect("grant");
    let content = port
        .authorize_content(&protected.token, Some(&grant), VISITOR, true)
        .await;
    let derived = port
        .authorize_derived(&protected.token, Some(&grant), VISITOR)
        .await;
    let wrong = port
        .verify_password(&protected.token, "wrong", VISITOR)
        .await;

    // Assert
    assert_eq!(
        opened.expect("open"),
        ShareOpen::Available {
            name: "photo.jpg".to_owned(),
            size: JPEG.size,
            media_type: Some(JPEG.media_type.to_owned()),
            expires_at: plain.expires_at,
        }
    );
    assert_eq!(content.expect("content"), content_of(&owner, photo));
    assert_eq!(
        derived.expect("derived"),
        ShareDerived {
            account_id: owner.account,
            file: expected,
        }
    );
    assert!(matches!(wrong, Err(PublicShareError::WrongPassword)));
    assert_eq!(
        accessed_events(&mut owner).await,
        1,
        "the content call only"
    );
    drop((port, shares));
    owner.finish().await;
    harness.finish().await;
}
