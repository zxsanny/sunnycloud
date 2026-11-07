use std::time::Duration;

use contracts::{
    ChangeFeedError, ChangePage, Cursor, Principal, PrincipalRevalidator, RevalidatorError,
};
use sc_clock::{Cancelled, MonotonicInstant};
use sc_observability::{NoLabels, log_warn};

use crate::change_feed::PgChangeFeed;
use crate::notifier::Subscription;

/// One parked read. No connection is held: each wake that needs the database takes a pooled connection for
/// one short query.
pub(crate) struct Hold<'a> {
    pub principal: &'a Principal,
    pub revalidator: &'a dyn PrincipalRevalidator,
    pub cursor: Cursor,
    pub limit: Option<u32>,
    pub started: MonotonicInstant,
    pub wait: Duration,
    pub subscription: Subscription,
}

enum Woken {
    Notified,
    Timer,
}

impl Hold<'_> {
    pub(crate) async fn park(mut self, feed: &PgChangeFeed) -> Result<ChangePage, ChangeFeedError> {
        let deadline = after(self.started, self.wait);
        let mut next_auth = after(self.started, feed.config.longpoll_auth_recheck);
        let mut next_safety = after(self.started, feed.config.feed_safety_recheck);
        loop {
            let next = deadline.min(next_auth).min(next_safety);
            let woken = tokio::select! {
                biased;
                () = feed.shutdown.cancelled() => return Ok(self.empty()),
                () = self.subscription.notified() => Woken::Notified,
                slept = feed.clock.sleep_until_monotonic(next) => match slept {
                    Ok(()) => Woken::Timer,
                    Err(Cancelled) => return Ok(self.empty()),
                },
            };
            match woken {
                Woken::Notified => {
                    if let Some(page) = self.changes(feed).await? {
                        return self.deliver(page).await;
                    }
                }
                Woken::Timer => {
                    let now = feed.clock.monotonic();
                    if now >= next_auth {
                        self.ensure_valid().await?;
                        next_auth = after(now, feed.config.longpoll_auth_recheck);
                    }
                    if now >= next_safety {
                        if let Some(page) = self.changes(feed).await? {
                            self.count_missed_wake(feed, &page);
                            return self.deliver(page).await;
                        }
                        next_safety = after(now, feed.config.feed_safety_recheck);
                    }
                    if now >= deadline {
                        return Ok(self.empty());
                    }
                }
            }
        }
    }

    fn empty(&self) -> ChangePage {
        ChangePage {
            items: Vec::new(),
            next_cursor: self.cursor,
            has_more: false,
        }
    }

    async fn changes(&self, feed: &PgChangeFeed) -> Result<Option<ChangePage>, ChangeFeedError> {
        let page = feed
            .reader
            .read_page(self.principal.account_id, self.cursor, self.limit)
            .await?;
        Ok((!page.items.is_empty()).then_some(page))
    }

    /// A safety recheck that finds changes no notify announced: a wake lost in process, or a commit made by
    /// another process. A notify that merely arrives while the recheck runs is counted too (a window of
    /// microseconds between COMMIT and `notify`).
    fn count_missed_wake(&self, feed: &PgChangeFeed, page: &ChangePage) {
        if page.next_cursor.seq > self.subscription.notified_seq() {
            feed.missed_wakes.inc(NoLabels);
            log_warn!(
                "safety recheck found changes a notify missed",
                account_id = self.principal.account_id
            );
        }
    }

    /// No item leaves a hold after the revalidator refused the principal.
    async fn deliver(&self, page: ChangePage) -> Result<ChangePage, ChangeFeedError> {
        self.ensure_valid().await?;
        Ok(page)
    }

    async fn ensure_valid(&self) -> Result<(), ChangeFeedError> {
        match self.revalidator.revalidate(self.principal).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ChangeFeedError::Unauthenticated),
            Err(RevalidatorError::Unexpected(fault)) => Err(ChangeFeedError::Unexpected(fault)),
        }
    }
}

fn after(at: MonotonicInstant, by: Duration) -> MonotonicInstant {
    at.checked_add(by).unwrap_or(at)
}
