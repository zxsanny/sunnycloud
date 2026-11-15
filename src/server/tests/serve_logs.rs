//! AZ-4318 AC-1 against the real `sunnycloud serve` binary: a request handler that fails with an internal error and
//! a janitor cycle that fails each leave a JSON log line on the binary's output. Both run on spawned tasks, which do
//! not inherit a subscriber attached to the command future. The failure is arranged from outside: the table the
//! upload start and the session reaper both read is renamed while `serve` runs.

mod root_support;
mod serve_support;

use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use serve_support::http::call;
use serve_support::{Server, Stack, json_lines};

const WITHIN: Duration = Duration::from_secs(10);
const HANDLER_FAILURE: &str = "unexpected failure answered as internal";

/// The log lines whose message `matches`, once at least one exists or `WITHIN` has passed.
fn wait_for_lines(server: &Server, matches: impl Fn(&str) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + WITHIN;
    loop {
        let found: Vec<Value> = json_lines(&server.lines())
            .into_iter()
            .filter(|line| line["msg"].as_str().is_some_and(&matches))
            .collect();
        if !found.is_empty() || Instant::now() >= deadline {
            return found;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_request_and_a_failing_janitor_cycle_each_leave_a_json_log_line() {
    // Arrange
    let mut stack = Stack::new().await;
    let account = stack.account().await;
    stack.root.setup.set("session_reap_interval", "1s");
    let mut server = stack.spawn();
    server.wait_ready(stack.ports.ops, WITHIN);
    let mut admin = stack.root.db.admin().await;
    sqlx::query("ALTER TABLE upload_sessions RENAME TO upload_sessions_gone")
        .execute(&mut admin)
        .await
        .expect("rename the table");

    // Act
    let started = call(
        stack.ports.public,
        "POST",
        "/api/v1/uploads",
        &account.token,
        &[("Content-Type", "application/json".to_owned())],
        json!({
            "parent_id": account.root,
            "name": "logs.bin",
            "declared_size": 1,
            "client_key": "serve-logs",
        })
        .to_string()
        .as_bytes(),
    );
    let handler = wait_for_lines(&server, |message| message == HANDLER_FAILURE);
    let janitor = wait_for_lines(&server, |message| {
        message.starts_with("janitor ") && message.ends_with(" cycle failed")
    });
    server.signal("TERM");
    let exited = server.wait_exit(WITHIN);

    // Assert
    assert_eq!(started.status, 500, "{started:?}");
    assert_eq!(handler.len(), 1, "{:#?}", exited.stderr);
    assert_eq!(handler[0]["level"], "error", "{:#?}", handler[0]);
    assert!(!janitor.is_empty(), "{:#?}", exited.stderr);
    assert_eq!(janitor[0]["level"], "error", "{:#?}", janitor[0]);
    stack.finish().await;
}
