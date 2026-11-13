//! AC-5 (AZ-3923, IT-20-2): 50 concurrent replacements from one ETag on a real PostgreSQL 18. Exactly one wins,
//! the others see `PreconditionFailed`, and the stored header is the winner's.

use contracts::{VaultError, VaultService};

use crate::service_support::Env;

const CALLS: usize = 50;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn fifty_concurrent_replaces_from_one_etag_have_exactly_one_winner() {
    // Arrange
    let mut env = Env::new().await;
    let (account, vault, e1) = env.account_with_vault(b"E1 header").await;

    // Act
    let calls: Vec<_> = (0..CALLS)
        .map(|i| {
            let service = env.service.clone();
            let if_match = e1.clone();
            tokio::spawn(async move {
                let header = format!("header of call {i}").into_bytes();
                let outcome = service
                    .replace_header(account, vault, &header, &if_match)
                    .await;
                (header, outcome)
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for call in calls {
        outcomes.push(call.await.expect("replace task"));
    }

    // Assert
    let winners: Vec<&Vec<u8>> = outcomes
        .iter()
        .filter(|(_, outcome)| outcome.is_ok())
        .map(|(header, _)| header)
        .collect();
    let stale = outcomes
        .iter()
        .filter(|(_, outcome)| matches!(outcome, Err(VaultError::PreconditionFailed)))
        .count();
    assert_eq!(winners.len(), 1, "{outcomes:?}");
    assert_eq!(stale, CALLS - 1, "{outcomes:?}");
    assert_eq!(env.stored(vault).await.as_ref(), Some(winners[0]));
    env.finish().await;
}
