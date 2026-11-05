use sqlx::{AssertSqlSafe, PgConnection, Postgres, Transaction};

use crate::error::DbError;
use crate::scope::AccountId;

const ADVISORY_LOCK_ACCOUNT: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('sunnycloud.account:' || $1::text, 0))";

/// The open transaction of one unit of work.
///
/// It has no public constructor: only [`crate::Db::run`] and [`crate::Db::run_system`] hand one out, so a
/// port that takes `&mut DbTx` cannot be called with a value built elsewhere.
///
/// ```compile_fail,E0423
/// let _tx = sc_db::DbTx;
/// ```
///
/// ```compile_fail,E0451
/// let _tx = sc_db::DbTx { depth: 0 };
/// ```
pub struct DbTx {
    tx: Transaction<'static, Postgres>,
    depth: u32,
}

impl DbTx {
    pub(crate) fn new(tx: Transaction<'static, Postgres>) -> Self {
        DbTx { tx, depth: 0 }
    }

    pub(crate) async fn commit(self) -> Result<(), DbError> {
        Ok(self.tx.commit().await?)
    }

    pub(crate) async fn rollback(self) -> Result<(), DbError> {
        Ok(self.tx.rollback().await?)
    }

    /// The connection of the transaction, for `sqlx::query(..).execute(tx.conn())`. Repository modules only;
    /// never issue `BEGIN`, `COMMIT` or `RESET` through it.
    pub fn conn(&mut self) -> &mut PgConnection {
        &mut self.tx
    }

    /// Runs `f` inside a savepoint. An error rolls back to the savepoint and is returned; the outer
    /// transaction stays usable. Savepoints nest.
    ///
    /// A failure of the savepoint statements themselves (a lost connection) is returned as the `DbError` it
    /// converts from, in place of the closure's error, because the transaction is gone.
    pub async fn savepoint<T, E>(
        &mut self,
        f: impl AsyncFnOnce(&mut DbTx) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<DbError>,
    {
        self.depth += 1;
        let name = format!("sc_savepoint_{}", self.depth);
        let result = self.run_savepoint(&name, f).await;
        self.depth -= 1;
        result
    }

    async fn run_savepoint<T, E>(
        &mut self,
        name: &str,
        f: impl AsyncFnOnce(&mut DbTx) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<DbError>,
    {
        self.statement(format!("SAVEPOINT {name}")).await?;
        match f(self).await {
            Ok(value) => {
                self.statement(format!("RELEASE SAVEPOINT {name}")).await?;
                Ok(value)
            }
            Err(error) => {
                self.statement(format!("ROLLBACK TO SAVEPOINT {name}"))
                    .await?;
                self.statement(format!("RELEASE SAVEPOINT {name}")).await?;
                Err(error)
            }
        }
    }

    // The text is built from a counter this type owns, never from caller input.
    async fn statement(&mut self, text: String) -> Result<(), DbError> {
        sqlx::query(AssertSqlSafe(text))
            .execute(&mut *self.tx)
            .await?;
        Ok(())
    }

    /// Takes the transaction-level advisory lock of one account (`pg_advisory_xact_lock`), released at commit
    /// or rollback. A second transaction for the same account waits here; other accounts do not.
    pub async fn advisory_lock_account(&mut self, account: AccountId) -> Result<(), DbError> {
        sqlx::query(ADVISORY_LOCK_ACCOUNT)
            .bind(account)
            .execute(&mut *self.tx)
            .await?;
        Ok(())
    }
}
