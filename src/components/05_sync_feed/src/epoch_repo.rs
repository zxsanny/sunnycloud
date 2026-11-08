use std::sync::Arc;

use contracts::{
    Cursor, EpochAdmin, EpochAdminError, EpochMarker, HeadReport, Marker, MarkerStatus, async_trait,
};
use sc_clock::Timestamp;
use sc_db::{Db, DbError, RunError};
use sc_observability::log_info;
use sqlx::Row;
use uuid::Uuid;

/// One statement, so the new epoch is chosen and written against the same rows: every row gets
/// `greatest(max(epoch) + 1, $1)` where `$1` is the clock's unix seconds. The result is `NULL` when there is no
/// row, in which case nothing changed.
const BUMP: &str = "\
WITH current AS (
    SELECT max(epoch) AS epoch FROM account_feeds
), bumped AS (
    UPDATE account_feeds SET epoch = GREATEST(current.epoch + 1, $1) FROM current
    RETURNING account_feeds.epoch
)
SELECT max(epoch) AS epoch FROM bumped";

const DB_EPOCH: &str = "SELECT COALESCE(max(epoch), 0) FROM account_feeds";

const HEADS: &str = "SELECT account_id, epoch, seq FROM account_feeds ORDER BY account_id";

/// A bump on an installation with no feed row: nothing to resync and no epoch to write.
#[derive(Debug, thiserror::Error)]
#[error("no account feed row to bump")]
pub struct NothingToBump;

/// The operator side of the epoch: bump, marker status and head report. Every call runs in system scope (the
/// rows of all accounts) in a short transaction of its own; the `.epoch` marker is reached only through
/// `EpochMarker`, never inside a transaction.
#[derive(Clone)]
pub struct PgEpochAdmin {
    db: Db,
    marker: Arc<dyn EpochMarker>,
}

impl PgEpochAdmin {
    pub fn new(db: Db, marker: Arc<dyn EpochMarker>) -> Self {
        PgEpochAdmin { db, marker }
    }
}

#[async_trait]
impl EpochAdmin for PgEpochAdmin {
    /// Commits the new epoch, then writes the marker. A marker failure is returned after the commit; the rerun
    /// bumps again and writes the marker.
    async fn bump_epoch(&self, now: Timestamp) -> Result<i64, EpochAdminError> {
        let unix_seconds = now.as_unix_micros().div_euclid(1_000_000);
        let epoch: Option<i64> = self
            .db
            .run_system("feed_epoch_bump", async |tx| {
                sqlx::query(BUMP)
                    .bind(unix_seconds)
                    .fetch_one(tx.conn())
                    .await?
                    .try_get("epoch")
                    .map_err(DbError::from)
            })
            .await
            .map_err(admin_error)?;
        let epoch = epoch.ok_or_else(|| EpochAdminError::Unexpected(Box::new(NothingToBump)))?;
        log_info!("epoch bumped");
        self.marker
            .write(epoch)
            .await
            .map_err(|error| EpochAdminError::Unexpected(Box::new(error)))?;
        Ok(epoch)
    }

    /// `db_epoch` is `max(epoch)`, 0 on an installation with no feed row.
    async fn marker_status(&self) -> Result<MarkerStatus, EpochAdminError> {
        let db_epoch = self
            .db
            .run_system("feed_epoch_status", async |tx| {
                sqlx::query_scalar(DB_EPOCH)
                    .fetch_one(tx.conn())
                    .await
                    .map_err(DbError::from)
            })
            .await
            .map_err(admin_error)?;
        let marker = self
            .marker
            .read()
            .await
            .map_err(|error| EpochAdminError::Unexpected(Box::new(error)))?
            .map_or(Marker::Absent, Marker::Present);
        Ok(MarkerStatus { db_epoch, marker })
    }

    async fn head_report(&self) -> Result<HeadReport, EpochAdminError> {
        let heads: Vec<(Uuid, Cursor)> = self
            .db
            .run_system("feed_head_report", async |tx| {
                let rows = sqlx::query(HEADS).fetch_all(tx.conn()).await?;
                rows.iter()
                    .map(|row| {
                        Ok((
                            row.try_get("account_id")?,
                            Cursor {
                                epoch: row.try_get("epoch")?,
                                seq: row.try_get("seq")?,
                            },
                        ))
                    })
                    .collect::<Result<_, sqlx::Error>>()
                    .map_err(DbError::from)
            })
            .await
            .map_err(admin_error)?;
        let max_epoch = heads.iter().map(|(_, head)| head.epoch).max().unwrap_or(0);
        Ok(HeadReport { max_epoch, heads })
    }
}

fn admin_error(error: RunError<DbError>) -> EpochAdminError {
    let (RunError::Closure(error) | RunError::Db(error)) = error;
    EpochAdminError::Unexpected(Box::new(error))
}
