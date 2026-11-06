//! AC-2, system scope: the API role sees the quota row of every account. `Db::run_system` is the only way to set
//! the scope, and `scripts/ci/check-db-gates.sh` admits a call only from a file on `run-system-allowlist.txt`.

use super::two_accounts;

#[tokio::test]
async fn system_scope_reads_the_quota_rows_of_every_account() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let mut visible: Vec<uuid::Uuid> = accounts
        .harness
        .api
        .run_system("quota_scope_probe", async |tx| {
            sqlx::query_scalar("SELECT account_id FROM quota")
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
    accounts.harness.finish().await;
}
