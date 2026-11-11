//! AC-5 (ST-05-5): a client that re-reads in a loop while a change is stamped every 20 ms gets no page with items
//! once its principal is revoked, and the loop ends with `Unauthenticated`. The scripted revalidator turns false
//! at 100 ms of the probe clock (`longpoll_auth_recheck` is 100 ms, the cache time of 01 is not in play); the
//! probe clock moves 20 ms per stamp, so the run takes no wall-clock time.
//!
//! The immediate path (a read that finds items at once) does not consult the revalidator: the HTTP layer
//! authenticates each request, and the session cache time of 01 bounds that path. Only the held path is
//! checked here.

use std::sync::{Arc, Mutex, PoisonError};

use contracts::{ChangeFeed, ChangeFeedError};
use sc_clock::{Clock, MonotonicInstant};

use crate::hold_support::{Rig, Scripted, config, cursor, eventually, ms, principal};

const STAMP_EVERY_MS: u64 = 20;
const REVOKE_AFTER_MS: u64 = 100;
const RECHECK_MS: u64 = 100;
const MAX_STEPS: usize = 50;

type Pages = Arc<Mutex<Vec<MonotonicInstant>>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_page_with_items_leaves_after_the_revoke_while_changes_keep_arriving() {
    // Arrange
    let rig = Rig::new(&config(RECHECK_MS, 10_000), 4).await;
    let account = rig.account().await;
    let me = principal(account.id);
    let started = rig.clock.monotonic();
    let revoked_at = started
        .checked_add(ms(REVOKE_AFTER_MS))
        .expect("instant in range");
    let revalidator = Arc::new(Scripted::revoked_at(&rig.clock, ms(REVOKE_AFTER_MS)));
    let pages: Pages = Pages::default();
    let client = tokio::spawn({
        let (feed, clock, who, pages) = (
            rig.feed.clone(),
            rig.clock.clone(),
            me.clone(),
            pages.clone(),
        );
        async move {
            let mut at = cursor(0);
            loop {
                match feed
                    .read(&who, revalidator.as_ref(), at, Some(ms(30_000)), None)
                    .await
                {
                    Ok(page) if page.items.is_empty() => panic!("the wait cannot expire here"),
                    Ok(page) => {
                        pages
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(clock.monotonic());
                        at = page.next_cursor;
                    }
                    Err(error) => return (error, clock.monotonic()),
                }
            }
        }
    });

    // Act
    let mut stamped = 0;
    while stamped < MAX_STEPS {
        eventually("the client is parked or ended", || {
            client.is_finished() || rig.feed.holds_of(me.session_or_device_id) == 1
        })
        .await;
        if client.is_finished() {
            break;
        }
        rig.clock.tick(ms(STAMP_EVERY_MS));
        rig.commit_and_notify(&account).await;
        stamped += 1;
        eventually("the client answered the stamp", || {
            client.is_finished()
                || pages.lock().unwrap_or_else(PoisonError::into_inner).len() == stamped
        })
        .await;
    }
    let (ended, ended_at) = client.await.expect("client task");

    // Assert
    let delivered = pages.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert!(
        matches!(ended, ChangeFeedError::Unauthenticated),
        "{ended:?}"
    );
    assert_eq!(delivered.len(), 4, "pages while the principal was valid");
    assert!(delivered.iter().all(|at| *at < revoked_at));
    assert!(ended_at.saturating_duration_since(revoked_at) <= ms(RECHECK_MS));
    rig.finish().await;
}
