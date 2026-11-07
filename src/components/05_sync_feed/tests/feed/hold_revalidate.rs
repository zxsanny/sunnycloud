//! AC-6 (IT-05-7 a): the revalidator ends a hold with `Unauthenticated`, and no item leaves a revoked hold. The
//! scripted revalidator turns false when the probe clock passes 150 ms; `longpoll_auth_recheck` is 100 ms.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use contracts::{ChangeFeedError, PrincipalRevalidator, RevalidatorError, async_trait};

use crate::hold_support::{Rig, Scripted, config, cursor, ms, outcome, principal, settle, valid};

const AUTH_RECHECK_MS: u64 = 100;

fn rig_config() -> sunnycloud_sync_feed::FeedConfig {
    config(AUTH_RECHECK_MS, 10_000)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revoked_hold_ends_with_unauthenticated_at_the_first_recheck_after_the_revoke_and_the_other_continues()
 {
    // Arrange
    let rig = Rig::new(&rig_config(), 4).await;
    let account = rig.account().await;
    let revalidator = Arc::new(Scripted::revoked_at(&rig.clock, ms(150)));
    let revoked = rig.read(
        &principal(account.id),
        revalidator.clone(),
        cursor(0),
        ms(30_000),
    );
    let steady = rig.read(&principal(account.id), valid(), cursor(0), ms(30_000));
    rig.clock.sleeps_reach(2).await;

    // Act
    rig.clock.tick(ms(100));
    rig.clock.sleeps_reach(4).await;
    let calls_before_the_revoke = revalidator.calls.load(Ordering::SeqCst);
    rig.clock.tick(ms(50));
    settle().await;
    let alive_at_150 = !revoked.is_finished();
    rig.clock.tick(ms(50));
    let ended = outcome(revoked).await;
    settle().await;

    // Assert
    assert_eq!(calls_before_the_revoke, 1);
    assert!(alive_at_150);
    assert!(matches!(ended, Err(ChangeFeedError::Unauthenticated)));
    assert!(!steady.is_finished());
    let seq = rig.commit_and_notify(&account).await;
    let page = outcome(steady)
        .await
        .expect("the valid holder still gets its page");
    assert_eq!(page.next_cursor, cursor(seq));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_change_that_wakes_a_revoked_hold_is_not_returned_to_it() {
    // Arrange
    let rig = Rig::new(&rig_config(), 4).await;
    let account = rig.account().await;
    let revoked = rig.read(
        &principal(account.id),
        Arc::new(Scripted::revoked_at(&rig.clock, ms(50))),
        cursor(0),
        ms(30_000),
    );
    let steady = rig.read(&principal(account.id), valid(), cursor(0), ms(30_000));
    rig.clock.sleeps_reach(2).await;
    rig.clock.tick(ms(50));
    settle().await;

    // Act
    let seq = rig.commit_and_notify(&account).await;
    let ended = outcome(revoked).await;
    let page = outcome(steady).await;

    // Assert
    assert!(matches!(ended, Err(ChangeFeedError::Unauthenticated)));
    assert_eq!(page.expect("page").next_cursor, cursor(seq));
    rig.finish().await;
}

struct Broken;

#[async_trait]
impl PrincipalRevalidator for Broken {
    async fn revalidate(
        &self,
        _principal: &contracts::Principal,
    ) -> Result<bool, RevalidatorError> {
        Err(RevalidatorError::Unexpected(Box::new(std::fmt::Error)))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failing_revalidator_ends_the_hold_with_the_unexpected_error() {
    // Arrange
    let rig = Rig::new(&rig_config(), 4).await;
    let account = rig.account().await;
    let held = rig.read(
        &principal(account.id),
        Arc::new(Broken),
        cursor(0),
        ms(30_000),
    );
    rig.clock.sleeps_reach(1).await;

    // Act
    rig.clock.tick(ms(AUTH_RECHECK_MS));
    let ended = outcome(held).await;

    // Assert
    assert!(matches!(ended, Err(ChangeFeedError::Unexpected(_))));
    rig.finish().await;
}
