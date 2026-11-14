//! IT-06-7 and IT-06-11 (AZ-3700 AC-5, AC-6): the expiry of `authorize_content` follows the injectable clock
//! (`expires_at > now` is live), a revoke answers `NotFound` on the very next call (no cache), and a failed
//! `share_accessed` write goes through the writer's best-effort path (counted and logged at ERROR there, as
//! `sc_audit`'s own tests assert; this binary's one global subscriber is the statement log) and never changes the
//! answer. `share_created` and `share_revoked`
//! stay fail closed (06 description §5, `sc_audit`): with the same failing writer they answer `Unexpected` and
//! change nothing.

use std::time::Duration;

use contracts::{PublicShareError, PublicShareService, ShareError, ShareService};

use crate::content_support::{Owner, content_of};
use crate::harness::Harness;
use crate::owner_support::{audit_rows, link_count, revoked_at_of};
use crate::visitor_support::{HOUR, VISITOR};

/// The API role may no longer insert into `audit_log`: every audit write fails (the `sc_audit` test precedent).
const BLOCK_AUDIT_INSERT: &str = "REVOKE INSERT ON audit_log FROM sunnycloud";

#[tokio::test]
async fn the_answer_follows_the_clock_across_the_expiry_and_a_revoke_on_the_next_call() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let shares = harness.shares();
    let expiring = shares
        .create(owner.account, file.node, Some(HOUR), None)
        .await
        .expect("create");
    let revoked = shares
        .create(owner.account, file.node, Some(24 * HOUR), None)
        .await
        .expect("create");
    let milli = Duration::from_millis(1);

    // Act
    harness.clock.tick(HOUR - milli);
    let before = shares
        .authorize_content(&expiring.token, None, VISITOR, false)
        .await;
    harness.clock.tick(milli);
    let at = shares
        .authorize_content(&expiring.token, None, VISITOR, false)
        .await;
    harness.clock.tick(Duration::from_secs(1));
    let after = shares
        .authorize_content(&expiring.token, None, VISITOR, false)
        .await;
    let live = shares
        .authorize_content(&revoked.token, None, VISITOR, false)
        .await;
    shares
        .revoke(owner.account, revoked.id)
        .await
        .expect("revoke");
    let after_revoke = shares
        .authorize_content(&revoked.token, None, VISITOR, false)
        .await;

    // Assert
    assert_eq!(before.expect("live before"), content_of(&owner, file));
    assert!(matches!(at, Err(PublicShareError::NotFound)), "{at:?}");
    assert!(
        matches!(after, Err(PublicShareError::NotFound)),
        "{after:?}"
    );
    assert_eq!(live.expect("live"), content_of(&owner, file));
    assert!(matches!(after_revoke, Err(PublicShareError::NotFound)));
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn a_failed_access_event_is_counted_as_a_best_effort_failure_and_the_access_is_still_allowed()
{
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let audit = harness.audit_writer();
    let shares = harness.shares_on(&harness.config, audit.clone());
    let link = shares
        .create(owner.account, file.node, None, None)
        .await
        .expect("create");
    harness.db.admin_exec(BLOCK_AUDIT_INSERT).await;

    // Act
    let answer = shares
        .authorize_content(&link.token, None, VISITOR, true)
        .await;

    // Assert
    assert_eq!(answer.expect("allowed"), content_of(&owner, file));
    assert_eq!(audit.stats().write_failures, 1);
    let accessed = audit_rows(owner.probe())
        .await
        .into_iter()
        .filter(|row| row.0 == "share_accessed")
        .count();
    assert_eq!(accessed, 0);
    drop((shares, audit));
    owner.finish().await;
    harness.finish().await;
}

#[tokio::test]
async fn create_and_revoke_stay_fail_closed_when_their_audit_write_fails() {
    // Arrange
    let harness = Harness::new().await;
    let mut owner = Owner::new(&harness).await;
    let file = owner.new_file(&harness, owner.root, "f.txt").await;
    let shares = harness.shares();
    let link = shares
        .create(owner.account, file.node, None, None)
        .await
        .expect("create");
    harness.db.admin_exec(BLOCK_AUDIT_INSERT).await;

    // Act
    let created = shares.create(owner.account, file.node, None, None).await;
    let revoked = shares.revoke(owner.account, link.id).await;

    // Assert
    assert!(
        matches!(created, Err(ShareError::Unexpected(_))),
        "{created:?}"
    );
    assert!(
        matches!(revoked, Err(ShareError::Unexpected(_))),
        "{revoked:?}"
    );
    assert_eq!(link_count(owner.probe()).await, 1, "the create rolled back");
    assert_eq!(revoked_at_of(owner.probe(), link.id).await, None);
    drop(shares);
    owner.finish().await;
    harness.finish().await;
}
