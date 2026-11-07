//! AC-1, AC-2, AC-3 and AC-7 (IT-05-6, IT-05-8): holds park without a connection, wake on notify, expire and end on
//! shutdown. The clock moves only when a test ticks it, so a return without a tick is a wake, not a timeout.

use contracts::ChangePage;
use sc_clock::Clock;
use std::time::Duration;

use crate::hold_support::{
    Acct, Held, Rig, config, cursor, eventually, ms, outcome, principal, settle, valid,
};

const ACCOUNTS: usize = 11;
const HOLDERS: usize = 100;
const POOL: u32 = 4;

struct Crowd {
    rig: Rig,
    accounts: Vec<Acct>,
    holders: Vec<(usize, Held)>,
}

/// 100 holders: 10 principals on account 0 and 9 on each of ten more accounts, all parked on an empty feed.
async fn crowd(wait: Duration) -> Crowd {
    let rig = Rig::new(&config(2_000, 10_000), POOL).await;
    let mut accounts = Vec::new();
    for _ in 0..ACCOUNTS {
        accounts.push(rig.account().await);
    }
    let mut holders = Vec::new();
    for index in 0..HOLDERS {
        let owner = if index < 10 { 0 } else { 1 + (index - 10) / 9 };
        let who = principal(accounts[owner].id);
        holders.push((owner, rig.read(&who, valid(), cursor(0), wait)));
    }
    rig.clock.sleeps_reach(HOLDERS).await;
    Crowd {
        rig,
        accounts,
        holders,
    }
}

impl Crowd {
    async fn release_all(self) {
        self.rig.shutdown.cancel();
        for (_, held) in self.holders {
            outcome(held).await.expect("empty page on shutdown");
        }
        self.rig.finish().await;
    }
}

fn only_item_seq(page: &ChangePage) -> (usize, i64, i64) {
    (page.items.len(), page.items[0].seq, page.next_cursor.seq)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hundred_parked_holds_use_no_connection_beyond_the_pool_of_four() {
    // Arrange
    let crowd = crowd(Duration::from_secs(300)).await;

    // Act
    eventually("every connection back in the pool", || {
        crowd.rig.fixture.api.stats().pool_in_use == 0
    })
    .await;
    let stats = crowd.rig.fixture.api.stats();
    let server_side = crowd.rig.fixture.db.open_connections().await;

    // Assert
    assert_eq!(stats.pool_max, POOL);
    assert_eq!(stats.pool_in_use, 0, "a parked hold holds no connection");
    assert!(server_side <= i64::from(POOL), "{server_side} connections");
    assert_eq!(crowd.rig.notifier.waiting_accounts(), ACCOUNTS);
    crowd.release_all().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_notify_wakes_that_accounts_holders_with_the_new_page_and_the_others_keep_waiting() {
    // Arrange
    let crowd = crowd(Duration::from_secs(300)).await;
    let woken = &crowd.accounts[0];
    let slept_before = crowd.rig.clock.monotonic();

    // Act
    let seq = crowd.rig.commit_and_notify(woken).await;
    let mut pages = Vec::new();
    let mut waiting = Vec::new();
    for (owner, held) in crowd.holders {
        if owner == 0 {
            pages.push(outcome(held).await.expect("page"));
        } else {
            waiting.push(held);
        }
    }
    settle().await;

    // Assert
    assert_eq!(pages.len(), 10);
    for page in &pages {
        assert_eq!(only_item_seq(page), (1, seq, seq));
    }
    assert_eq!(waiting.len(), 90);
    assert!(waiting.iter().all(|held| !held.is_finished()));
    assert_eq!(crowd.rig.clock.monotonic(), slept_before, "no tick moved");
    assert_eq!(crowd.rig.missed.get(sc_observability::NoLabels), 0);
    crowd.rig.shutdown.cancel();
    for held in waiting {
        outcome(held).await.expect("empty page on shutdown");
    }
    crowd.rig.finish().await;
}

#[tokio::test]
async fn an_unchanged_hold_returns_an_empty_page_with_the_same_cursor_when_its_wait_expires() {
    // Arrange
    let rig = Rig::new(&config(2_000, 10_000), POOL).await;
    let account = rig.account().await;
    let who = principal(account.id);
    let held = rig.read(&who, valid(), cursor(0), ms(300));
    rig.clock.sleeps_reach(1).await;

    // Act
    rig.clock.tick(ms(299));
    settle().await;
    let before_expiry = held.is_finished();
    rig.clock.tick(ms(1));
    let page = outcome(held).await.expect("page");

    // Assert
    assert!(!before_expiry);
    assert!(page.items.is_empty());
    assert_eq!(page.next_cursor, cursor(0));
    assert!(!page.has_more);
    assert_eq!(rig.notifier.waiting_accounts(), 0);
    assert_eq!(rig.feed.holds_of(who.session_or_device_id), 0);
    rig.finish().await;
}

#[tokio::test]
async fn a_wait_above_the_configured_maximum_is_capped_to_it() {
    // Arrange
    let rig = Rig::new(&config(2_000, 10_000), POOL).await;
    let account = rig.account().await;
    let held = rig.read(
        &principal(account.id),
        valid(),
        cursor(0),
        Duration::from_secs(3_600),
    );
    rig.clock.sleeps_reach(1).await;

    // Act
    rig.clock.tick(rig.feed_wait_max());
    let page = outcome(held).await.expect("page");

    // Assert
    assert!(page.items.is_empty());
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_returns_a_hundred_thirty_second_holds_at_once_as_empty_pages_with_their_cursors()
{
    // Arrange
    let crowd = crowd(Duration::from_secs(30)).await;

    // Act
    crowd.rig.shutdown.cancel();
    let mut pages = Vec::new();
    for (_, held) in crowd.holders {
        pages.push(outcome(held).await.expect("page"));
    }

    // Assert
    assert_eq!(pages.len(), HOLDERS);
    assert!(pages.iter().all(|page| page.items.is_empty()));
    assert!(pages.iter().all(|page| page.next_cursor == cursor(0)));
    assert_eq!(crowd.rig.notifier.waiting_accounts(), 0);
    crowd.rig.finish().await;
}
