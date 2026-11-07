use std::sync::Arc;
use std::time::Duration;

use contracts::{
    ChangeFeed, ChangeFeedError, ChangePage, Cursor, Principal, PrincipalRevalidator, async_trait,
};
use sc_clock::{CancellationToken, Clock};
use sc_observability::{Counter, NoLabels};
use uuid::Uuid;

use crate::config::FeedConfig;
use crate::hold::Hold;
use crate::hold_cap::HoldCap;
use crate::notifier::InProcessNotifier;
use crate::read_repo::PgFeedReader;

/// The `Retry-After` of a refused hold. Proposed, not yet agreed with the owner; a constant, not a config key.
pub const PROPOSED_HOLD_RETRY_AFTER: Duration = Duration::from_secs(5);

/// `ChangeFeed` over PostgreSQL: the immediate read of `PgFeedReader`, and the hold when that read is empty and
/// the caller wants to wait. Holds park on `notifier` without a connection.
pub struct PgChangeFeed {
    pub(crate) reader: PgFeedReader,
    notifier: Arc<InProcessNotifier>,
    holds: Arc<HoldCap>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) shutdown: CancellationToken,
    pub(crate) missed_wakes: Arc<Counter<NoLabels>>,
    pub(crate) config: FeedConfig,
}

impl PgChangeFeed {
    /// `shutdown` is the application's token: cancelling it returns every held read as an empty page.
    pub fn new(
        reader: PgFeedReader,
        notifier: Arc<InProcessNotifier>,
        clock: Arc<dyn Clock>,
        shutdown: CancellationToken,
        missed_wakes: Arc<Counter<NoLabels>>,
        config: &FeedConfig,
    ) -> Self {
        PgChangeFeed {
            reader,
            notifier,
            holds: HoldCap::new(config.max_holds_per_principal),
            clock,
            shutdown,
            missed_wakes,
            config: config.clone(),
        }
    }

    /// Reads currently parked for the principal (a session or device id).
    pub fn holds_of(&self, principal: Uuid) -> u64 {
        self.holds.holding(principal)
    }
}

#[async_trait]
impl ChangeFeed for PgChangeFeed {
    async fn read(
        &self,
        principal: &Principal,
        revalidator: &dyn PrincipalRevalidator,
        cursor: Cursor,
        wait: Option<Duration>,
        limit: Option<u32>,
    ) -> Result<ChangePage, ChangeFeedError> {
        let account = principal.account_id;
        let wait = wait.map_or(Duration::ZERO, |wait| {
            wait.min(self.config.longpoll_wait_max)
        });
        if wait.is_zero() {
            return self.reader.read_page(account, cursor, limit).await;
        }
        let started = self.clock.monotonic();
        let subscription = self.notifier.subscribe(account);
        let first = self.reader.read_page(account, cursor, limit).await?;
        if !first.items.is_empty() {
            return Ok(first);
        }
        let permit = self.holds.acquire(principal.session_or_device_id).ok_or(
            ChangeFeedError::TooManyHolds {
                retry_after: PROPOSED_HOLD_RETRY_AFTER,
            },
        )?;
        let page = Hold {
            principal,
            revalidator,
            cursor,
            limit,
            started,
            wait,
            subscription,
        }
        .park(self)
        .await;
        drop(permit);
        page
    }

    async fn head(&self, account: Uuid) -> Result<Cursor, ChangeFeedError> {
        self.reader.head(account).await
    }
}
