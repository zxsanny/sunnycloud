//! The system-scope statement of the real-implementation suite (AZ-3720): an account created as 01 creates it,
//! minus the identity and invite rows, so the real hooks' `initialize` runs in one system-scope transaction.

use std::sync::Arc;

use contracts::AccountLifecycleHook;
use uuid::Uuid;

use crate::support::{Fixture, START};

/// Same text as 01's `INSERT_ACCOUNT`.
const INSERT_ACCOUNT: &str = "INSERT INTO accounts (id, state, created_at, display_name, role) \
    VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', $3, $4)";

/// An account as 01's account creation makes it, minus the identity and invite rows: the account row, then every
/// hook's `initialize`, in one system-scope transaction. Returns the account and its root.
pub async fn create_account(
    fixture: &Fixture,
    hooks: &[Arc<dyn AccountLifecycleHook>],
) -> (Uuid, Uuid) {
    let account = Uuid::new_v4();
    fixture
        .api
        .run_system("account_create", async |tx| {
            sqlx::query(INSERT_ACCOUNT)
                .bind(account)
                .bind(START.as_unix_micros())
                .bind("Fixture")
                .bind("user")
                .execute(tx.conn())
                .await
                .map_err(|error| contracts::LifecycleError::Unexpected(Box::new(error)))?;
            for hook in hooks {
                hook.initialize(tx, account).await?;
            }
            Ok::<_, contracts::LifecycleError>(())
        })
        .await
        .expect("create the account");
    let root = fixture
        .db
        .admin_count(&format!(
            "SELECT count(*) FROM nodes WHERE account_id = '{account}' AND parent_id IS NULL"
        ))
        .await;
    assert_eq!(root, 1, "initialize made one root");
    let mut admin = fixture.db.admin().await;
    let root: Uuid = sqlx::query_scalar("SELECT id FROM nodes WHERE account_id = $1")
        .bind(account)
        .fetch_one(&mut admin)
        .await
        .expect("root id");
    crate::mutation_support::close(admin).await;
    (account, root)
}
