//! AZ-3795 AC-5: `admin audit export --since` over the real audit reader and a real database, two pages.

mod support;

use std::sync::Arc;
use std::time::Duration;

use sc_audit::{AuditEvent, AuditEventKind, AuditReader, AuditWriter, EXPORT_PAGE_ROWS};
use sc_clock::{ManualClock, Timestamp};
use sc_testdb::{START, TestDb};
use serde_json::Value;
use support::Harness;

const BEFORE: usize = 3;

async fn record(db: &TestDb, at: Timestamp, count: usize) {
    let api = db.api().await;
    let writer = AuditWriter::new(api, Arc::new(ManualClock::new(at)));
    for _ in 0..count {
        writer
            .record(AuditEvent::new(AuditEventKind::SignIn))
            .await
            .expect("seed row");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_prints_every_row_since_the_time_as_one_json_line_across_two_pages() {
    // Arrange
    let db = TestDb::clone_template().await;
    let since = START;
    record(
        &db,
        since.checked_sub(Duration::from_secs(60)).expect("range"),
        BEFORE,
    )
    .await;
    let after = EXPORT_PAGE_ROWS + 5;
    record(
        &db,
        since.checked_add(Duration::from_secs(60)).expect("range"),
        after,
    )
    .await;
    let reader = Arc::new(AuditReader::new(db.api().await));
    let harness = Harness::new()
        .real_handler()
        .service::<AuditReader, _, _>(move |_| {
            let reader = Arc::clone(&reader);
            async move { Ok(reader) }
        });
    let since_text = "2026-11-07T12:00:00Z";
    assert_eq!(
        since.as_unix_micros(),
        1_794_052_800_000_000,
        "the harness start is {since_text}"
    );

    // Act
    let result = tokio::task::spawn_blocking(move || {
        harness.run(&["admin", "audit", "export", "--since", since_text])
    })
    .await
    .expect("the command thread");

    // Assert
    assert_eq!(result.exit, 0, "{}", result.stderr);
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert_eq!(lines.len(), after, "only the rows since the time");
    for line in &lines {
        let row: Value = serde_json::from_str(line).expect("one JSON object per line");
        assert_eq!(row["event"], "sign_in");
        assert!(row.get("ip").is_none(), "no address in the export");
    }
    db.drop_db().await;
}

#[test]
fn a_since_that_is_not_utc_exits_3_with_no_read() {
    let result =
        Harness::new()
            .real_handler()
            .run(&["admin", "audit", "export", "--since", "2026-11-01"]);

    assert_eq!(result.exit, 3, "{}", result.stderr);
    assert_eq!(result.stdout, "");
}
