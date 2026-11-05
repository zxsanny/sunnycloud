//! Health and the read-only stats snapshot (inputs of the E-09 collectors).

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use sqlx::PgPool;

use crate::db::Db;
use crate::error::DbError;
use crate::migrate::MigrationSet;

/// Result of [`Db::health`]. `reachable = false` means no connection answered within the acquire timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbHealth {
    pub reachable: bool,
    /// `server_version` of the server; `None` when not reachable.
    pub server_version: Option<String>,
    /// Embedded migrations not yet applied; `None` when the bookkeeping table cannot be read.
    pub migrations_pending: Option<usize>,
    /// Connections checked out of the pool at the moment of the call, not counting the probe's own.
    pub pool_in_use: u32,
}

/// Count and time of the units of work (`Db::run`, `Db::run_system`) of one `op` label since start.
/// `Db::run` reports under `run`; `Db::run_system` under the label it was called with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryStats {
    pub op: &'static str,
    pub count: u64,
    pub total: Duration,
    pub max: Duration,
}

/// Snapshot of [`Db::stats`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbStats {
    pub pool_in_use: u32,
    pub pool_idle: u32,
    pub pool_max: u32,
    pub queries: Vec<QueryStats>,
    /// Pending count the last status read, health probe or migration run saw; `None` before the first.
    pub migrations_pending: Option<usize>,
}

#[derive(Debug, Default)]
struct State {
    pending: Option<usize>,
    queries: BTreeMap<&'static str, QueryStats>,
}

/// Process-local counters behind `Db::stats`. Nothing here is persisted or needed for correctness.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    state: Mutex<State>,
}

impl Stats {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set_pending(&self, pending: usize) {
        self.state().pending = Some(pending);
    }

    pub(crate) fn record(&self, op: &'static str, elapsed: Duration) {
        let mut state = self.state();
        let entry = state.queries.entry(op).or_insert(QueryStats {
            op,
            count: 0,
            total: Duration::ZERO,
            max: Duration::ZERO,
        });
        entry.count += 1;
        entry.total += elapsed;
        entry.max = entry.max.max(elapsed);
    }
}

fn pool_in_use(pool: &PgPool) -> u32 {
    let idle = u32::try_from(pool.num_idle()).unwrap_or(u32::MAX);
    pool.size().saturating_sub(idle)
}

impl Db {
    /// Probes the database with one connection and reads the pending migration count. Never fails and never
    /// waits longer than `db_acquire_timeout`: an unreachable database is `reachable = false`.
    pub async fn health(&self) -> DbHealth {
        let in_use = pool_in_use(&self.pool);
        let unreachable = DbHealth {
            reachable: false,
            server_version: None,
            migrations_pending: None,
            pool_in_use: in_use,
        };
        match tokio::time::timeout(self.acquire_timeout, self.probe()).await {
            Ok(Ok((server_version, migrations_pending))) => DbHealth {
                reachable: true,
                server_version: Some(server_version),
                migrations_pending,
                pool_in_use: in_use,
            },
            Ok(Err(_)) | Err(_) => unreachable,
        }
    }

    async fn probe(&self) -> Result<(String, Option<usize>), DbError> {
        let mut conn = self.pool.acquire().await?;
        let version: String = sqlx::query_scalar("SELECT current_setting('server_version')")
            .fetch_one(&mut *conn)
            .await?;
        let pending = MigrationSet::embedded()
            .status_on(&self.stats, &mut conn)
            .await
            .map(|status| status.pending.len())
            .ok();
        Ok((version, pending))
    }

    /// Read-only snapshot of the pool and the recorded counters.
    pub fn stats(&self) -> DbStats {
        let state = self.stats.state();
        DbStats {
            pool_in_use: pool_in_use(&self.pool),
            pool_idle: u32::try_from(self.pool.num_idle()).unwrap_or(u32::MAX),
            pool_max: self.pool.options().get_max_connections(),
            queries: state.queries.values().copied().collect(),
            migrations_pending: state.pending,
        }
    }
}
