//! The system-scope statement of the transfer integration pass (AZ-3760): an account created as 01 creates it,
//! minus the identity and invite rows (01's sign-in that runs it needs a live OIDC provider), so the real hooks'
//! `initialize` runs in one system-scope transaction as in 01.

use std::sync::Arc;

use contracts::{AccountLifecycleHook, LifecycleError};
use sc_db::Db;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::support::START;

/// Same text as 01's `INSERT_ACCOUNT`.
const INSERT_ACCOUNT: &str = "INSERT INTO accounts (id, state, created_at, display_name, role) \
    VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', $3, $4)";

/// The account row, then every hook's `initialize`, in one system-scope transaction; returns the account and the
/// root `initialize` made, read on the owner connection `owner`.
pub async fn create_account(
    api: &Db,
    owner: &mut PgConnection,
    hooks: &[Arc<dyn AccountLifecycleHook>],
) -> (Uuid, Uuid) {
    let account = Uuid::new_v4();
    api.run_system("account_create", async |tx| {
        sqlx::query(INSERT_ACCOUNT)
            .bind(account)
            .bind(START.as_unix_micros())
            .bind("Fixture")
            .bind("user")
            .execute(tx.conn())
            .await
            .map_err(|error| LifecycleError::Unexpected(Box::new(error)))?;
        for hook in hooks {
            hook.initialize(tx, account).await?;
        }
        Ok::<_, LifecycleError>(())
    })
    .await
    .expect("create the account");
    let roots: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM nodes WHERE account_id = $1 AND parent_id IS NULL")
            .bind(account)
            .fetch_all(owner)
            .await
            .expect("root of the account");
    assert_eq!(roots.len(), 1, "initialize made one root");
    (account, roots[0])
}
