use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::config::{DbConfig, DbRole};
use crate::error::{DbError, RunError};
use crate::roles;
use crate::scope::{self, Scope};
use crate::stats::Stats;
use crate::tx::DbTx;

/// The only owner of PostgreSQL connections. Cheap to clone; clones share one pool.
#[derive(Debug, Clone)]
pub struct Db {
    pub(crate) pool: PgPool,
    pub(crate) role: DbRole,
    pub(crate) acquire_timeout: Duration,
    pub(crate) stats: Arc<Stats>,
}

impl Db {
    /// Opens the pool for `role` and verifies the connected role (`DbError::WrongRole`). The pool applies
    /// `db_statement_timeout` and `db_idle_in_tx_timeout` to every connection it opens.
    pub async fn connect(cfg: &DbConfig, role: DbRole) -> Result<Db, DbError> {
        let options = cfg
            .url(role)?
            .parse::<PgConnectOptions>()
            .map_err(|_| DbError::Other("the database URL is not a valid postgres URL".into()))?
            .options([
                (
                    "statement_timeout",
                    cfg.db_statement_timeout.as_millis().to_string(),
                ),
                (
                    "idle_in_transaction_session_timeout",
                    cfg.db_idle_in_tx_timeout.as_millis().to_string(),
                ),
            ]);
        let pool = PgPoolOptions::new()
            .max_connections(cfg.db_pool_max)
            .acquire_timeout(cfg.db_acquire_timeout)
            .connect_with(options)
            .await?;
        if let Err(error) = roles::verify(&pool, role).await {
            pool.close().await;
            return Err(error);
        }
        Ok(Db {
            pool,
            role,
            acquire_timeout: cfg.db_acquire_timeout,
            stats: Arc::default(),
        })
    }

    /// One unit of work in one short transaction scoped to `scope`: the scope is the first statement, `Ok`
    /// commits, `Err` rolls back and is returned as [`RunError::Closure`], a panic rolls back as the
    /// transaction drops. The closure must not await anything but the database.
    pub async fn run<T, E>(
        &self,
        scope: Scope,
        f: impl AsyncFnOnce(&mut DbTx) -> Result<T, E>,
    ) -> Result<T, RunError<E>> {
        let Scope::Account(account) = scope;
        let started = Instant::now();
        let result = async {
            let mut tx = self.begin().await?;
            scope::apply_account(tx.conn(), account).await?;
            Self::finish(tx, f).await
        }
        .await;
        self.stats.record("run", started.elapsed());
        result
    }

    /// Like [`Db::run`] in system scope (all accounts). `op` names the operation in `pg_stat_activity`
    /// (`application_name`). Call sites are limited to the reviewed allowlist (`scripts/ci/check-db-gates.sh`).
    pub async fn run_system<T, E>(
        &self,
        op: &'static str,
        f: impl AsyncFnOnce(&mut DbTx) -> Result<T, E>,
    ) -> Result<T, RunError<E>> {
        let started = Instant::now();
        let result = async {
            let mut tx = self.begin().await?;
            scope::apply_system(tx.conn(), op).await?;
            Self::finish(tx, f).await
        }
        .await;
        self.stats.record(op, started.elapsed());
        result
    }

    async fn begin(&self) -> Result<DbTx, DbError> {
        Ok(DbTx::new(self.pool.begin().await?))
    }

    // A failed rollback means the connection is gone: the server rolls back when it closes and the pool
    // discards the connection. That is returned as `Db(Unavailable)` in place of the closure's error, because
    // the caller retries Unavailable and the closure's outcome no longer matters.
    async fn finish<T, E>(
        mut tx: DbTx,
        f: impl AsyncFnOnce(&mut DbTx) -> Result<T, E>,
    ) -> Result<T, RunError<E>> {
        match f(&mut tx).await {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(RunError::Closure(error))
            }
        }
    }
}
