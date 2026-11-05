//! Tenant and system scope: the only file in the workspace that names the scope settings
//! (`scripts/ci/check-db-gates.sh` fails when another crate does).

use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DbError;

/// The account `Uuid` the ports already use.
pub type AccountId = Uuid;

/// Tenant scope of a [`crate::Db::run`] call. System scope is reachable only through
/// [`crate::Db::run_system`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Account(AccountId),
}

const SET_ACCOUNT: &str = "SELECT set_config('sunnycloud.account_id', $1, true)";

// `application_name` carries the operation label, so a long system transaction is named in `pg_stat_activity`.
const SET_SYSTEM: &str = "SELECT set_config('sunnycloud.scope', 'system', true), set_config('application_name', $1, true)";

/// First statement of every transaction. Both settings are transaction-local, so commit, rollback or a
/// killed connection leaves nothing behind on the pooled connection.
pub(crate) async fn apply_account(
    conn: &mut PgConnection,
    account: AccountId,
) -> Result<(), DbError> {
    sqlx::query(SET_ACCOUNT)
        .bind(account.to_string())
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn apply_system(conn: &mut PgConnection, op: &'static str) -> Result<(), DbError> {
    sqlx::query(SET_SYSTEM).bind(op).execute(conn).await?;
    Ok(())
}
