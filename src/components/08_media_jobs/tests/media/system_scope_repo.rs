//! AC-3, system scope: the API role sees the `jobs`, `derived` and `worker_status` rows of every account.
//! `Db::run_system` is the only way to set the scope, and `scripts/ci/check-db-gates.sh` admits a call only from
//! a file on `run-system-allowlist.txt`.

use uuid::Uuid;

use crate::plans::{
    assert_claim_plan, assert_count_plan, claim_sql, count_sql, explain, seed_queue,
};
use crate::rls::two_accounts;

#[tokio::test]
async fn system_scope_reads_the_rows_of_every_account_in_all_three_tables() {
    // Arrange
    let accounts = two_accounts().await;

    // Act
    let (mut jobs, mut derived_accounts, workers): (Vec<Uuid>, Vec<Uuid>, Vec<String>) = accounts
        .fixture
        .api
        .run_system("media_scope_probe", async |tx| {
            let jobs = sqlx::query_scalar("SELECT id FROM jobs")
                .fetch_all(tx.conn())
                .await?;
            let derived = sqlx::query_scalar("SELECT account_id FROM derived")
                .fetch_all(tx.conn())
                .await?;
            let workers =
                sqlx::query_scalar("SELECT worker_id FROM worker_status ORDER BY worker_id")
                    .fetch_all(tx.conn())
                    .await?;
            Ok::<_, sqlx::Error>((jobs, derived, workers))
        })
        .await
        .expect("system read");

    // Assert
    jobs.sort();
    derived_accounts.sort();
    let mut expected_jobs = vec![accounts.job_a, accounts.job_b];
    expected_jobs.sort();
    let mut expected_accounts = vec![accounts.a, accounts.b];
    expected_accounts.sort();
    assert_eq!(jobs, expected_jobs);
    assert_eq!(derived_accounts, expected_accounts);
    assert_eq!(workers, ["worker-1", "worker-2"]);
    accounts.fixture.finish().await;
}

#[tokio::test]
async fn the_claim_and_the_count_keep_their_indexes_under_the_row_policy_in_system_scope() {
    // Arrange
    let queue = seed_queue().await;

    // Act
    let (image, video, count) = queue
        .fixture
        .api
        .run_system("media_plan_probe", async |tx| {
            let image = explain(tx.conn(), &claim_sql("image")).await;
            let video = explain(tx.conn(), &claim_sql("video")).await;
            let count = explain(tx.conn(), &count_sql(queue.account)).await;
            Ok::<_, sqlx::Error>((image, video, count))
        })
        .await
        .expect("system plans");

    // Assert
    assert_claim_plan(&image, "image");
    assert_claim_plan(&video, "video");
    assert_count_plan(&count);
    queue.fixture.finish().await;
}
