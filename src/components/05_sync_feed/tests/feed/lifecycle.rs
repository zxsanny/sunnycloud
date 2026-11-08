//! AC-3 and AC-4 (IT-05-13): the feed row at account creation and at erase on a real PostgreSQL 18. The marker
//! is a double of the `EpochMarker` port (the file implementation is 04's and 05 may not depend on it).

use std::sync::Arc;

use contracts::{
    AccountLifecycleHook, ChangeFeedError, EpochMarker, PrincipalRevalidator, RevalidatorError,
    async_trait,
};
use sc_clock::ManualClock;
use sc_db::{Db, Scope};
use sqlx::Connection;
use sunnycloud_sync_feed::PgFeedLifecycle;
use uuid::Uuid;

use crate::hold_support::{Rig, config, cursor, ms, outcome, principal};
use crate::marker_double::MarkerDouble;
use crate::support::{Fixture, START, account};
use crate::system_scope_repo::initialize_in_system_scope;

const CLOCK_SECONDS: i64 = 1_794_052_800;

/// Reports the account valid while its feed row exists, so an erase ends the hold at the next recheck.
struct RowPresent {
    db: Db,
    account: Uuid,
}

#[async_trait]
impl PrincipalRevalidator for RowPresent {
    async fn revalidate(
        &self,
        _principal: &contracts::Principal,
    ) -> Result<bool, RevalidatorError> {
        self.db
            .run(Scope::Account(self.account), async |tx| {
                sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM account_feeds WHERE account_id = $1)",
                )
                .bind(self.account)
                .fetch_one(tx.conn())
                .await
                .map_err(sc_db::DbError::from)
            })
            .await
            .map_err(|error| RevalidatorError::Unexpected(Box::new(error)))
    }
}

struct Hooked {
    fixture: Fixture,
    hook: PgFeedLifecycle,
    marker: Arc<MarkerDouble>,
}

impl Hooked {
    async fn new(marker: MarkerDouble) -> Hooked {
        let fixture = Fixture::new().await;
        let marker = Arc::new(marker);
        let hook = PgFeedLifecycle::new(
            fixture.api.clone(),
            Arc::new(ManualClock::new(START)),
            marker.clone(),
        );
        Hooked {
            fixture,
            hook,
            marker,
        }
    }

    /// The hook holds a clone of the pool; it goes before the fixture checks for leaked connections.
    async fn finish(self) {
        let Hooked { fixture, hook, .. } = self;
        drop(hook);
        fixture.finish().await;
    }

    async fn new_account(&self) -> Uuid {
        let mut conn = self.fixture.owner_conn().await;
        let id = Uuid::new_v4();
        account(&mut conn, id).await;
        conn.close().await.expect("close owner connection");
        id
    }

    async fn epoch_of(&self, account: Uuid) -> Option<i64> {
        let mut conn = self.fixture.owner_conn().await;
        let epoch = sqlx::query_scalar("SELECT epoch FROM account_feeds WHERE account_id = $1")
            .bind(account)
            .fetch_optional(&mut conn)
            .await
            .expect("read the epoch");
        conn.close().await.expect("close owner connection");
        epoch
    }

    async fn initialize(&self, account: Uuid) -> Result<(), contracts::LifecycleError> {
        initialize_in_system_scope(&self.fixture.api, &self.hook, account).await
    }
}

#[tokio::test]
async fn the_first_account_of_an_install_gets_the_clock_seconds_as_epoch_and_the_marker_holds_it() {
    // Arrange
    let hooked = Hooked::new(MarkerDouble::default()).await;
    let account = hooked.new_account().await;

    // Act
    hooked.initialize(account).await.expect("initialize");

    // Assert
    assert_eq!(hooked.epoch_of(account).await, Some(CLOCK_SECONDS));
    assert_eq!(
        hooked.marker.read().await.expect("read"),
        Some(CLOCK_SECONDS)
    );
    hooked.finish().await;
}

#[tokio::test]
async fn a_later_account_gets_the_epoch_of_the_existing_rows_and_leaves_the_marker_alone() {
    // Arrange
    let hooked = Hooked::new(MarkerDouble::default()).await;
    let (first, second) = (hooked.new_account().await, hooked.new_account().await);
    hooked.initialize(first).await.expect("first");
    let mut conn = hooked.fixture.owner_conn().await;
    sqlx::query("UPDATE account_feeds SET epoch = 1794052900")
        .execute(&mut conn)
        .await
        .expect("an epoch bump");
    conn.close().await.expect("close owner connection");

    // Act
    hooked.initialize(second).await.expect("second");

    // Assert
    assert_eq!(hooked.epoch_of(second).await, Some(1_794_052_900));
    assert_eq!(hooked.marker.writes(), 1);
    assert_eq!(hooked.marker.value(), Some(CLOCK_SECONDS));
    hooked.finish().await;
}

#[tokio::test]
async fn a_failed_marker_write_fails_initialize_and_leaves_no_row() {
    // Arrange
    let hooked = Hooked::new(MarkerDouble::failing()).await;
    let account = hooked.new_account().await;

    // Act
    let result = hooked.initialize(account).await;

    // Assert
    assert!(result.is_err());
    assert_eq!(hooked.epoch_of(account).await, None);
    hooked.finish().await;
}

#[tokio::test]
async fn erase_batch_removes_the_row_reports_nothing_remaining_and_repeats_the_same() {
    // Arrange
    let hooked = Hooked::new(MarkerDouble::default()).await;
    let account = hooked.new_account().await;
    hooked.initialize(account).await.expect("initialize");

    // Act
    let first = hooked
        .hook
        .erase_batch(account, 100)
        .await
        .expect("first erase");
    let second = hooked
        .hook
        .erase_batch(account, 100)
        .await
        .expect("second erase");

    // Assert
    assert_eq!((first.remaining, second.remaining), (0, 0));
    assert_eq!(hooked.epoch_of(account).await, None);
    hooked.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_parked_read_ends_with_unauthenticated_once_the_account_is_erased() {
    // Arrange
    let rig = Rig::new(&config(100, 10_000), 4).await;
    let account = rig.account().await;
    let marker = Arc::new(MarkerDouble::default());
    let hook = PgFeedLifecycle::new(rig.fixture.api.clone(), rig.clock.clone(), marker);
    let revalidator = Arc::new(RowPresent {
        db: rig.fixture.api.clone(),
        account: account.id,
    });
    let held = rig.read(&principal(account.id), revalidator, cursor(0), ms(30_000));
    rig.clock.sleeps_reach(1).await;

    // Act
    hook.erase_batch(account.id, 100).await.expect("erase");
    rig.clock.tick(ms(100));
    let ended = outcome(held).await;

    // Assert
    assert!(matches!(ended, Err(ChangeFeedError::Unauthenticated)));
    drop(hook);
    rig.finish().await;
}
