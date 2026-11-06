//! AC-5 (PT-01-1), functional part: 100 principals on 16 tasks, repeat tokens with a cache refresh every 20th
//! call. Off the reference server the timing targets are NOT-RUN(HW-REF); this run proves a 0% error rate,
//! correct principals and a bounded number of database connections.

mod support;

use std::sync::Arc;
use std::time::Duration;

use contracts::{Authenticator, Principal};
use sqlx::Connection;
use support::auth::{FAR_FUTURE, authenticator, session};
use support::fixtures;
use support::{Harness, START};
use uuid::Uuid;

const PRINCIPALS: usize = 100;
const TASKS: usize = 16;
const CALLS_PER_TASK: usize = 250;
const REFRESH_EVERY: usize = 20;
const MAX_CONNECTIONS: i64 = 8;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sixteen_tasks_over_one_hundred_principals_see_no_error() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let at = START.as_unix_micros();
    let mut expected = Vec::new();
    for index in 0..PRINCIPALS {
        let account = Uuid::new_v4();
        fixtures::account(&mut conn, account, at).await;
        let token = format!("token-{index}");
        let id =
            fixtures::live_session(&mut conn, account, &token, "browser", FAR_FUTURE, at).await;
        expected.push((token, account, id));
    }
    conn.close().await.expect("close fixture connection");
    let authenticator = Arc::new(authenticator(&harness));
    let expected = Arc::new(expected);

    // Act
    let tasks: Vec<_> = (0..TASKS)
        .map(|task| {
            let authenticator = authenticator.clone();
            let expected = expected.clone();
            let clock = harness.clock.clone();
            tokio::spawn(async move {
                let mut failures: Vec<String> = Vec::new();
                for call in 0..CALLS_PER_TASK {
                    if call % REFRESH_EVERY == 0 {
                        clock.tick(Duration::from_millis(250));
                    }
                    let (token, account, id) = &expected[(task * 7 + call) % PRINCIPALS];
                    match authenticator.authenticate(session(token)).await {
                        Ok(Principal {
                            account_id,
                            session_or_device_id,
                            ..
                        }) if account_id == *account && session_or_device_id == *id => {}
                        other => failures.push(format!("{token}: {other:?}")),
                    }
                }
                failures
            })
        })
        .collect();
    let mut failures = Vec::new();
    for task in tasks {
        failures.extend(task.await.expect("task finished"));
    }
    let connections = harness.db.open_connections().await;

    // Assert
    assert_eq!(failures, Vec::<String>::new());
    assert!(connections <= MAX_CONNECTIONS, "{connections} connections");
    drop(authenticator);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
