//! AZ-3798: `scrub run` and `scrub status` through the real handler with a scripted `Scrubber`.

mod key_support;
mod scrub_support;
mod support;

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    ByteRate, KeyError, MasterKeyring, ScrubMode, ScrubStatus, ScrubSummary, Scrubber,
};
use sc_clock::Timestamp;
use scrub_support::{Scripted, as_service, step};
use serde_json::Value;
use sunnycloud_server::CliSettings;
use support::{Captured, Harness};

const PROGRESS: Duration = Duration::from_millis(500);

fn run(scrubber: &Arc<Scripted>, args: &[&str]) -> Captured {
    run_with_keyring(scrubber, &Arc::new(key_support::Scripted::new()), args)
}

fn run_with_keyring(
    scrubber: &Arc<Scripted>,
    keyring: &Arc<key_support::Scripted>,
    args: &[&str],
) -> Captured {
    let service = as_service(scrubber);
    let keys = key_support::as_service(keyring);
    Harness::new()
        .real_handler()
        .settings(CliSettings {
            progress_interval: PROGRESS,
            ..CliSettings::default()
        })
        .service::<dyn Scrubber, _, _>(move |_| {
            let service = Arc::clone(&service);
            async move { Ok(service) }
        })
        .service::<dyn MasterKeyring, _, _>(move |_| {
            let keys = Arc::clone(&keys);
            async move { Ok(keys) }
        })
        .run(args)
}

#[test]
fn full_with_a_rate_and_plain_runs_call_start_once_step_until_done_and_finish_once() {
    // Arrange
    let steps = || {
        vec![
            step(10, 0, 0, false),
            step(5, 0, 0, false),
            step(0, 0, 0, true),
        ]
    };
    let full = Arc::new(Scripted::new(steps()));
    let due = Arc::new(Scripted::new(steps()));

    // Act
    let full_out = run(&full, &["scrub", "run", "--full", "--rate", "5"]);
    let due_out = run(&due, &["scrub", "run"]);

    // Assert: `Due` with no rate lets 04 apply `scrub_max_rate`.
    assert_eq!(
        (full_out.exit, due_out.exit),
        (0, 0),
        "{}{}",
        full_out.stderr,
        due_out.stderr
    );
    assert_eq!(
        full.calls(),
        (
            vec![(ScrubMode::Full, Some(ByteRate(5 * 1024 * 1024)))],
            3,
            1
        )
    );
    assert_eq!(due.calls(), (vec![(ScrubMode::Due, None)], 3, 1));
}

#[test]
fn exit_is_0_without_findings_and_2_for_a_mismatch_or_a_missing_blob_with_the_counts_printed() {
    for (script, exit, counts) in [
        (
            vec![step(7, 0, 0, false), step(3, 0, 0, true)],
            0,
            "checked: 10\ninterrupted: false\nmismatched: 0\nmissing: 0\n",
        ),
        (
            vec![step(7, 1, 0, false), step(3, 0, 0, true)],
            2,
            "checked: 10\ninterrupted: false\nmismatched: 1\nmissing: 0\n",
        ),
        (
            vec![step(7, 0, 0, false), step(3, 0, 1, true)],
            2,
            "checked: 10\ninterrupted: false\nmismatched: 0\nmissing: 1\n",
        ),
    ] {
        // Arrange
        let scrubber = Arc::new(Scripted::new(script));

        // Act
        let out = run(&scrubber, &["scrub", "run"]);

        // Assert
        assert_eq!(out.exit, exit, "{}", out.stderr);
        assert_eq!(out.stdout, counts);
    }
}

#[test]
fn a_progress_line_appears_on_stderr_at_least_every_interval_while_a_step_runs() {
    // Arrange: two steps of 1.2 s each, 500 ms between progress lines.
    let scrubber = Arc::new(
        Scripted::new(vec![step(1, 0, 0, false), step(1, 0, 0, true)])
            .delay(Duration::from_millis(1200)),
    );

    // Act
    let out = run(&scrubber, &["scrub", "run"]);

    // Assert: 2 ticks inside each step, plus one line after each step.
    assert_eq!(out.exit, 0, "{}", out.stderr);
    let lines = out
        .stderr
        .lines()
        .filter(|line| line.starts_with("scrub: "))
        .count();
    assert!(lines >= 6, "{} progress lines: {}", lines, out.stderr);
}

#[test]
fn status_prints_the_same_facts_in_text_and_json() {
    // Arrange
    let scrubber = Arc::new(Scripted::new(Vec::new()).status(ScrubStatus {
        oldest_scrubbed_at: Some(Timestamp::from_unix_micros(1_700_000_000_000_000)),
        unscrubbed: 12,
        latest_summary: Some(ScrubSummary {
            checked: 900,
            mismatched: 1,
            missing: 2,
            duration: Duration::from_millis(4500),
        }),
        unavailable: 3,
    }));

    // Act
    let text = run(&scrubber, &["scrub", "status"]);
    let json = run(&scrubber, &["--json", "scrub", "status"]);

    // Assert
    assert_eq!(
        (text.exit, json.exit),
        (0, 0),
        "{}{}",
        text.stderr,
        json.stderr
    );
    let data =
        serde_json::from_str::<Value>(&json.stdout).expect("one JSON object")["data"].clone();
    let facts: Vec<(&str, &str)> = text
        .stdout
        .lines()
        .map(|line| line.split_once(": ").expect("key: value"))
        .collect();
    assert_eq!(facts.len(), 7);
    for (key, value) in facts {
        assert_eq!(data[key].to_string(), value, "{key}");
    }
    assert_eq!(data["oldest_scrubbed_at_micros"], 1_700_000_000_000_000_i64);
    assert_eq!(data["unavailable"], 3);
    assert_eq!(data["last_run_duration_ms"], 4500);
}

#[test]
fn a_status_with_no_run_yet_prints_null_for_the_missing_facts() {
    // Arrange
    let scrubber = Arc::new(Scripted::new(Vec::new()));

    // Act
    let out = run(&scrubber, &["--json", "scrub", "status"]);

    // Assert
    let data = serde_json::from_str::<Value>(&out.stdout).expect("one JSON object")["data"].clone();
    assert_eq!(out.exit, 0, "{}", out.stderr);
    assert_eq!(
        (
            &data["oldest_scrubbed_at_micros"],
            &data["last_run_checked"]
        ),
        (&Value::Null, &Value::Null)
    );
}

#[test]
fn a_rate_of_zero_negative_or_not_a_number_exits_3_with_no_start_call() {
    for rate in ["0", "-1", "fast"] {
        // Arrange
        let scrubber = Arc::new(Scripted::new(vec![step(1, 0, 0, true)]));

        // Act
        let out = run(&scrubber, &["scrub", "run", "--rate", rate]);

        // Assert
        assert_eq!(out.exit, 3, "{rate}: {}", out.stderr);
        assert_eq!(scrubber.calls(), (Vec::new(), 0, 0), "{rate}");
    }
}

#[test]
fn a_run_checks_the_key_ring_first_and_a_refusal_exits_2_with_no_start_call() {
    // Arrange
    let scrubber = Arc::new(Scripted::new(vec![step(1, 0, 0, true)]));
    let keyring = Arc::new(key_support::Scripted::new().verify(|| Err(KeyError::FileMissing)));

    // Act
    let out = run_with_keyring(&scrubber, &keyring, &["scrub", "run", "--full"]);

    // Assert
    assert_eq!(out.exit, 2, "{}", out.stderr);
    assert!(
        out.stderr.contains("master key file missing"),
        "{}",
        out.stderr
    );
    assert_eq!(scrubber.calls(), (Vec::new(), 0, 0));
}
