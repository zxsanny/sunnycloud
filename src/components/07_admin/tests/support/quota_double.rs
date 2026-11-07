//! A double of 07a's `QuotaAdmin` over the real `quota` table, because 07 may not link the quota crate
//! (ADR 015, `crate_graph`). It keeps the contract rules 07 relies on: `views` omits unknown ids, `set_limit`
//! accepts 0 to 2^53 or `None` and answers `NotFound` for an unknown account and `InvalidLimit` for anything
//! else, and `effective_limit` falls back to the installation default.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use contracts::{Drift, QuotaAdmin, QuotaAdminError, QuotaView, async_trait};
use sc_db::{Db, Scope};
use uuid::Uuid;

pub const DEFAULT_LIMIT: i64 = 10 << 30;
const MAX_LIMIT: i64 = 1 << 53;

type QuotaRow = (Uuid, Option<i64>, i64, i64);

pub struct QuotaDouble {
    reader: Db,
    pub view_calls: AtomicUsize,
    pub views_calls: AtomicUsize,
    pub max_views_ids: AtomicUsize,
    pub set_limit_calls: AtomicUsize,
}

impl QuotaDouble {
    /// `reader` is a handle of the schema owner, which row-level security does not hide rows from.
    pub fn new(reader: Db) -> Self {
        QuotaDouble {
            reader,
            view_calls: AtomicUsize::new(0),
            views_calls: AtomicUsize::new(0),
            max_views_ids: AtomicUsize::new(0),
            set_limit_calls: AtomicUsize::new(0),
        }
    }

    async fn rows(&self, accounts: Vec<Uuid>) -> Result<Vec<QuotaRow>, QuotaAdminError> {
        self.reader
            .run(Scope::Account(Uuid::nil()), async |tx| {
                sqlx::query_as::<_, QuotaRow>(
                    "SELECT account_id, limit_bytes, used_bytes, reserved_bytes FROM quota \
                     WHERE account_id = ANY($1)",
                )
                .bind(accounts)
                .fetch_all(tx.conn())
                .await
            })
            .await
            .map_err(database)
    }
}

fn database(error: impl std::error::Error + Send + Sync + 'static) -> QuotaAdminError {
    QuotaAdminError::Unexpected(Box::new(error))
}

fn view_of((_, limit, used, reserved): QuotaRow) -> QuotaView {
    QuotaView {
        limit_bytes: limit,
        effective_limit: limit.unwrap_or(DEFAULT_LIMIT),
        used,
        reserved,
    }
}

#[async_trait]
impl QuotaAdmin for QuotaDouble {
    async fn view(&self, account: Uuid) -> Result<QuotaView, QuotaAdminError> {
        self.view_calls.fetch_add(1, Ordering::SeqCst);
        self.rows(vec![account])
            .await?
            .into_iter()
            .next()
            .map(view_of)
            .ok_or(QuotaAdminError::NotFound)
    }

    async fn views(&self, accounts: &[Uuid]) -> Result<HashMap<Uuid, QuotaView>, QuotaAdminError> {
        self.views_calls.fetch_add(1, Ordering::SeqCst);
        self.max_views_ids
            .fetch_max(accounts.len(), Ordering::SeqCst);
        Ok(self
            .rows(accounts.to_vec())
            .await?
            .into_iter()
            .map(|row| (row.0, view_of(row)))
            .collect())
    }

    async fn set_limit(&self, account: Uuid, limit: Option<i64>) -> Result<(), QuotaAdminError> {
        self.set_limit_calls.fetch_add(1, Ordering::SeqCst);
        if limit.is_some_and(|bytes| !(0..=MAX_LIMIT).contains(&bytes)) {
            return Err(QuotaAdminError::InvalidLimit);
        }
        let changed = self
            .reader
            .run(Scope::Account(account), async |tx| {
                sqlx::query("UPDATE quota SET limit_bytes = $2 WHERE account_id = $1")
                    .bind(account)
                    .bind(limit)
                    .execute(tx.conn())
                    .await
                    .map(|done| done.rows_affected())
            })
            .await
            .map_err(database)?;
        if changed == 0 {
            return Err(QuotaAdminError::NotFound);
        }
        Ok(())
    }

    async fn recompute(
        &self,
        _account: Uuid,
        _live_bytes: i64,
        _reserved_bytes: i64,
        _repair: bool,
    ) -> Result<Drift, QuotaAdminError> {
        Err(QuotaAdminError::Unexpected(
            "the user administration does not call this port".into(),
        ))
    }
}
