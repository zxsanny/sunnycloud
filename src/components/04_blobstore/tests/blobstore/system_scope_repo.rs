//! AC-3, system scope: the API role sees the rows of every account. `Db::run_system` is the only way to set the
//! scope, and `scripts/ci/check-db-gates.sh` admits a call only from a file on `run-system-allowlist.txt`.

use crate::rls::two_accounts;

const COUNTS: &str = "SELECT (SELECT count(*) FROM blobs), (SELECT count(*) FROM blob_gc)";

#[tokio::test]
async fn system_scope_reads_the_rows_of_every_account() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let counts: (i64, i64) = accounts
        .fixture
        .api
        .run_system("blobstore_scope_probe", async |tx| {
            sqlx::query_as(COUNTS).fetch_one(tx.conn()).await
        })
        .await
        .expect("system read");

    // Assert
    assert_eq!(counts, (4, 2));
    accounts.fixture.finish().await;
}
