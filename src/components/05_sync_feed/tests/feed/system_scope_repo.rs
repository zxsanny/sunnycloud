//! AC-2, system scope: the API role sees the feed row of every account. `Db::run_system` is the only way to set
//! the scope, and `scripts/ci/check-db-gates.sh` admits a call only from a file on `run-system-allowlist.txt`.

use uuid::Uuid;

use crate::rls::two_accounts;

#[tokio::test]
async fn system_scope_reads_the_feed_rows_of_every_account() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let mut visible: Vec<Uuid> = accounts
        .fixture
        .api
        .run_system("feed_scope_probe", async |tx| {
            sqlx::query_scalar("SELECT account_id FROM account_feeds")
                .fetch_all(tx.conn())
                .await
        })
        .await
        .expect("system read");

    // Assert
    visible.sort();
    let mut expected = vec![accounts.a, accounts.b];
    expected.sort();
    assert_eq!(visible, expected);
    accounts.fixture.finish().await;
}

/// Creates the feed row of `account` the way account creation does: `initialize` in a system-scope
/// transaction, which sees the feed rows of every account.
pub async fn initialize_in_system_scope(
    db: &sc_db::Db,
    hook: &dyn contracts::AccountLifecycleHook,
    account: Uuid,
) -> Result<(), contracts::LifecycleError> {
    db.run_system("feed_initialize", async |tx| {
        hook.initialize(tx, account).await
    })
    .await
    .map_err(|error| match error {
        sc_db::RunError::Closure(error) => error,
        sc_db::RunError::Db(error) => contracts::LifecycleError::Unexpected(Box::new(error)),
    })
}
