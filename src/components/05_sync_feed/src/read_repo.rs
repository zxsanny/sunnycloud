use contracts::{ChangeFeedError, ChangeItem, ChangePage, Cursor, NodeKind, NodeState};
use sc_db::{Db, RunError, Scope};
use sc_ids::FileHash;
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::config::FeedConfig;
use crate::cursor::{FeedState, judge_cursor, page_limit};
use crate::error::{bad_node_row, feed_row_missing, read_error};

/// One statement, so the cursor is judged against the same snapshot the page comes from (a purge committing
/// between two statements could otherwise hide tombstones from a cursor that looked valid).
///
/// `cut` is the `seq` of the `limit`-th changed node; the page takes every node up to it, so the batch that
/// shares the last `seq` is never split. The guard in `cut` only avoids reading a page for a cursor that is
/// about to be refused: the decision itself is `judge_cursor` on the returned feed row. `more` is whether a
/// node newer than the page exists (an index probe for the first one, not a scan). A feed row always yields at least one row; none means no feed row.
pub const READ_PAGE_SQL: &str = "\
WITH feed AS (
    SELECT epoch, seq, purge_horizon FROM account_feeds WHERE account_id = $1
), cut AS (
    SELECT max(c.seq) AS seq FROM (
        SELECT n.seq FROM nodes n, feed f
        WHERE n.account_id = $1 AND n.seq > $3
          AND $2 = f.epoch AND $3 >= f.purge_horizon AND $3 <= f.seq
        ORDER BY n.seq LIMIT $4
    ) c
), newer AS (
    SELECT count(*) > 0 AS more FROM (
        SELECT 1 FROM nodes m WHERE m.account_id = $1 AND m.seq > (SELECT seq FROM cut)
        ORDER BY m.seq LIMIT 1
    ) one
)
SELECT feed.epoch AS feed_epoch, feed.seq AS feed_seq, feed.purge_horizon, newer.more,
       n.id, n.parent_id, n.kind, n.name, n.deleted_at IS NOT NULL AS tombstone,
       n.current_revision_id, r.size, r.content_hash, n.seq
FROM feed CROSS JOIN cut CROSS JOIN newer
LEFT JOIN nodes n ON n.account_id = $1 AND n.seq > $3 AND n.seq <= cut.seq
LEFT JOIN revisions r ON r.id = n.current_revision_id AND r.account_id = n.account_id
ORDER BY n.seq, n.id";

const HEAD_SQL: &str = "SELECT epoch, seq FROM account_feeds WHERE account_id = $1";

/// The immediate read path of the change feed: pages and head, one short transaction each.
#[derive(Debug, Clone)]
pub struct PgFeedReader {
    db: Db,
    page_default: u64,
    page_ceiling: Option<u64>,
}

struct Fetched {
    feed: FeedState,
    more: bool,
    items: Vec<ChangeItem>,
}

impl PgFeedReader {
    pub fn new(db: Db, config: &FeedConfig) -> Self {
        PgFeedReader {
            db,
            page_default: config.feed_page_limit_default,
            page_ceiling: config.feed_page_limit_max,
        }
    }

    /// The latest state of every node newer than `cursor`, or `ResyncRequired` when the cursor cannot be
    /// trusted. A page holds more than `limit` items when one `seq` batch crosses the limit.
    pub async fn read_page(
        &self,
        account: Uuid,
        cursor: Cursor,
        limit: Option<u32>,
    ) -> Result<ChangePage, ChangeFeedError> {
        let limit = page_limit(limit, self.page_default, self.page_ceiling);
        let fetched = self
            .db
            .run(Scope::Account(account), async |tx| {
                let rows = sqlx::query(READ_PAGE_SQL)
                    .bind(account)
                    .bind(cursor.epoch)
                    .bind(cursor.seq)
                    .bind(limit)
                    .fetch_all(tx.conn())
                    .await
                    .map_err(|error| read_error(error.into()))?;
                decode(account, &rows)
            })
            .await
            .map_err(run_error)?;
        judge_cursor(cursor, fetched.feed)
            .map_err(|reason| ChangeFeedError::ResyncRequired { reason })?;
        let next_cursor = fetched.items.last().map_or(cursor, |last| Cursor {
            epoch: cursor.epoch,
            seq: last.seq,
        });
        Ok(ChangePage {
            items: fetched.items,
            next_cursor,
            has_more: fetched.more,
        })
    }

    /// The account's current `epoch.seq`.
    pub async fn head(&self, account: Uuid) -> Result<Cursor, ChangeFeedError> {
        let head = self
            .db
            .run(Scope::Account(account), async |tx| {
                sqlx::query(HEAD_SQL)
                    .bind(account)
                    .fetch_optional(tx.conn())
                    .await
                    .map_err(|error| read_error(error.into()))?
                    .map(|row| {
                        Ok(Cursor {
                            epoch: row.try_get("epoch")?,
                            seq: row.try_get("seq")?,
                        })
                    })
                    .transpose()
                    .map_err(|error: sqlx::Error| read_error(error.into()))
            })
            .await
            .map_err(run_error)?;
        head.ok_or_else(|| feed_row_missing(account))
    }
}

fn run_error(error: RunError<ChangeFeedError>) -> ChangeFeedError {
    match error {
        RunError::Closure(error) => error,
        RunError::Db(error) => read_error(error),
    }
}

fn decode(account: Uuid, rows: &[PgRow]) -> Result<Fetched, ChangeFeedError> {
    let first = rows.first().ok_or_else(|| feed_row_missing(account))?;
    let feed = FeedState {
        epoch: column(first.try_get("feed_epoch"))?,
        seq: column(first.try_get("feed_seq"))?,
        purge_horizon: column(first.try_get("purge_horizon"))?,
    };
    let more = column(first.try_get("more"))?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let id: Option<Uuid> = column(row.try_get("id"))?;
        let Some(node_id) = id else { continue };
        items.push(item(account, node_id, row)?);
    }
    Ok(Fetched { feed, more, items })
}

fn item(account: Uuid, node_id: Uuid, row: &PgRow) -> Result<ChangeItem, ChangeFeedError> {
    let kind = match column(row.try_get::<String, _>("kind"))?.as_str() {
        "file" => NodeKind::File,
        "folder" => NodeKind::Folder,
        _ => return Err(bad_node_row(account, "kind")),
    };
    let tombstone: bool = column(row.try_get("tombstone"))?;
    let hash: Option<Vec<u8>> = column(row.try_get("content_hash"))?;
    let content_hash = hash
        .map(|bytes| <[u8; 32]>::try_from(bytes).map(FileHash::from_bytes))
        .transpose()
        .map_err(|_| bad_node_row(account, "content hash"))?;
    Ok(ChangeItem {
        node_id,
        parent_id: column(row.try_get("parent_id"))?,
        kind,
        name: column(row.try_get("name"))?,
        state: if tombstone {
            NodeState::Tombstone
        } else {
            NodeState::Live
        },
        revision_id: column(row.try_get("current_revision_id"))?,
        size: column(row.try_get("size"))?,
        content_hash,
        seq: column(row.try_get("seq"))?,
    })
}

fn column<T>(result: Result<T, sqlx::Error>) -> Result<T, ChangeFeedError> {
    result.map_err(|error| read_error(error.into()))
}
