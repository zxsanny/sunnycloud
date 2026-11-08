use std::sync::Arc;

use contracts::{AccountLifecycleHook, EpochMarker, EraseProgress, LifecycleError, async_trait};
use sc_clock::Clock;
use sc_db::{Db, DbError, DbTx, RunError, Scope};
use sqlx::Row;
use uuid::Uuid;

use crate::error::lifecycle_error;

/// One statement, so the choice of the epoch and the insert see the same rows: every row carries the
/// installation epoch, `max(epoch)` of the existing rows, or `$2` (the clock's unix seconds) on an empty
/// install. `first` is true in that second case.
const INSERT: &str = "\
WITH current AS (
    SELECT max(epoch) AS epoch FROM account_feeds
), inserted AS (
    INSERT INTO account_feeds (account_id, epoch)
    SELECT $1, COALESCE(current.epoch, $2) FROM current
    RETURNING epoch
)
SELECT inserted.epoch, current.epoch IS NULL AS first FROM inserted CROSS JOIN current";

const DELETE: &str = "DELETE FROM account_feeds WHERE account_id = $1";

/// The feed's part of the account lifecycle: the feed row at creation, with the installation epoch, and its
/// removal at erase.
///
/// `initialize` reads the epoch of the other accounts' rows, which only a transaction in system scope sees
/// (row-level security); the caller (01, account creation) runs it there.
#[derive(Clone)]
pub struct PgFeedLifecycle {
    db: Db,
    clock: Arc<dyn Clock>,
    marker: Arc<dyn EpochMarker>,
}

impl PgFeedLifecycle {
    pub fn new(db: Db, clock: Arc<dyn Clock>, marker: Arc<dyn EpochMarker>) -> Self {
        PgFeedLifecycle { db, clock, marker }
    }
}

#[async_trait]
impl AccountLifecycleHook for PgFeedLifecycle {
    async fn initialize(&self, tx: &mut DbTx, account: Uuid) -> Result<(), LifecycleError> {
        let unix_seconds = self.clock.now_utc().as_unix_micros().div_euclid(1_000_000);
        let row = sqlx::query(INSERT)
            .bind(account)
            .bind(unix_seconds)
            .fetch_one(tx.conn())
            .await
            .map_err(|error| lifecycle_error(error.into()))?;
        let epoch: i64 = row
            .try_get("epoch")
            .map_err(|error| lifecycle_error(error.into()))?;
        let first: bool = row
            .try_get("first")
            .map_err(|error| lifecycle_error(error.into()))?;
        if first {
            self.marker
                .write(epoch)
                .await
                .map_err(|error| LifecycleError::Unexpected(Box::new(error)))?;
        }
        Ok(())
    }

    async fn erase_batch(
        &self,
        account: Uuid,
        _limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        self.db
            .run(Scope::Account(account), async |tx| {
                sqlx::query(DELETE)
                    .bind(account)
                    .execute(tx.conn())
                    .await
                    .map(drop)
                    .map_err(DbError::from)
            })
            .await
            .map_err(|error| match error {
                RunError::Closure(error) | RunError::Db(error) => lifecycle_error(error),
            })?;
        Ok(EraseProgress { remaining: 0 })
    }
}
