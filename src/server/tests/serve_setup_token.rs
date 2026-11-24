//! AZ-3654 AC-1 and AC-4 (the `serve` callsite of the setup token) against the real `sunnycloud serve` binary on
//! PostgreSQL 18: an install without accounts prints one setup token on stdout and stores only its hash; a restart
//! replaces it; `--json` and an install with an account print none; no log line ever holds it.

mod root_support;
mod serve_support;

use std::time::Duration;

use serve_support::{Exited, Stack};
use sqlx::Connection;

const WITHIN: Duration = Duration::from_secs(10);
const EXIT_WITHIN: Duration = Duration::from_secs(10);

/// Starts `serve` with `args`, waits for `/readyz`, ends it with SIGTERM and returns what it wrote.
fn run_to_ready(stack: &Stack, args: &[&str]) -> Exited {
    let mut server = stack.spawn_with(args);
    server.wait_ready(stack.ports.ops, WITHIN);
    server.signal("TERM");
    let exited = server.wait_exit(EXIT_WITHIN);
    assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
    exited
}

/// The values of the `token:` lines of a text-mode stdout.
fn tokens(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("token: "))
        .map(str::to_owned)
        .collect()
}

/// Open setup invites whose stored hash is the SHA-256 of `token`, and all open setup invites.
async fn setup_rows(stack: &Stack, token: &str) -> (i64, i64) {
    let mut admin = stack.root.db.admin().await;
    let matching: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM invites WHERE kind = 'setup' AND redeemed_at IS NULL \
         AND token_hash = sha256($1::bytea)",
    )
    .bind(token.as_bytes())
    .fetch_one(&mut admin)
    .await
    .expect("matching setup invites");
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM invites WHERE kind = 'setup' AND redeemed_at IS NULL",
    )
    .fetch_one(&mut admin)
    .await
    .expect("open setup invites");
    admin.close().await.expect("close admin connection");
    (matching, open)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_install_prints_one_setup_token_and_stores_only_its_hash() {
    // Arrange
    let stack = Stack::new().await;

    // Act
    let exited = run_to_ready(&stack, &["serve"]);
    let printed = tokens(&exited.stdout);

    // Assert
    assert_eq!(printed.len(), 1, "stdout:\n{}", exited.stdout);
    let token = &printed[0];
    assert!(!token.is_empty());
    assert_eq!(setup_rows(&stack, token).await, (1, 1));
    assert!(
        !exited.stderr_text().contains(token.as_str()),
        "the token reached a log line"
    );
    assert!(
        exited
            .messages()
            .iter()
            .any(|message| message == "serve start: setup token")
    );
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restart_prints_a_new_token_and_the_old_one_is_gone() {
    // Arrange
    let stack = Stack::new().await;
    let first = tokens(&run_to_ready(&stack, &["serve"]).stdout);

    // Act
    let second = tokens(&run_to_ready(&stack, &["serve"]).stdout);

    // Assert
    assert_eq!((first.len(), second.len()), (1, 1));
    assert_ne!(first[0], second[0]);
    assert_eq!(setup_rows(&stack, &first[0]).await, (0, 1));
    assert_eq!(setup_rows(&stack, &second[0]).await, (1, 1));
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn json_mode_prints_no_token_and_issues_none() {
    // Arrange
    let stack = Stack::new().await;

    // Act
    let exited = run_to_ready(&stack, &["--json", "serve"]);

    // Assert
    assert!(!exited.stdout.contains("token"), "{}", exited.stdout);
    assert_eq!(setup_rows(&stack, "").await.1, 0);
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_install_with_an_account_prints_no_token() {
    // Arrange
    let stack = Stack::new().await;
    stack.account().await;

    // Act
    let exited = run_to_ready(&stack, &["serve"]);

    // Assert
    assert!(tokens(&exited.stdout).is_empty(), "{}", exited.stdout);
    assert_eq!(setup_rows(&stack, "").await.1, 0);
    stack.finish().await;
}
