//! AC-2 (size limit before insert), AC-3 (fail closed), AC-4 (best effort is visible) and AC-6 (mirror line)
//! on a real PostgreSQL 18 with the real API role.

mod support;

use std::net::IpAddr;
use std::time::Duration;

use sc_audit::{
    AuditDetail, AuditError, AuditEvent, AuditEventKind, MAX_DETAIL_BYTES, Rejection, SubjectRef,
};
use sc_clock::Clock;
use sc_db::{DbError, RunError, Scope};
use support::logs::LogCapture;
use support::{Fixture, START, event};
use uuid::Uuid;

const BUSINESS_TABLE: &str = "CREATE TABLE business_probe (id uuid PRIMARY KEY); \
    GRANT INSERT, SELECT ON business_probe TO sunnycloud";
const BLOCK_AUDIT_INSERT: &str = "REVOKE INSERT ON audit_log FROM sunnycloud";

/// event, actor account, actor session, subject type, subject id, address, detail text, `at` in microseconds.
type StoredRow = (
    String,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    String,
    i64,
);

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

/// A detail whose `jsonb` text is exactly `target` bytes: an integer entry with a key of `n` bytes adds `n + 7`
/// (`, "` + key + `": 1`), and the braces replace the first separator. The stored size is asserted from the
/// database, not from this arithmetic.
fn detail_of(target: usize) -> AuditDetail {
    let mut lengths = Vec::new();
    let mut remaining = target;
    while remaining > 71 + 8 {
        lengths.push(64);
        remaining -= 71;
    }
    if remaining > 71 {
        lengths.push(30);
        remaining -= 37;
    }
    lengths.push(remaining - 7);
    let mut detail = AuditDetail::new();
    for (index, length) in lengths.into_iter().enumerate() {
        detail = detail.with_int(leak(&format!("{index:03}{}", "k".repeat(length - 3))), 1);
    }
    detail
}

#[tokio::test]
async fn record_in_and_record_store_every_field_and_stamp_at_from_the_clock() {
    let fx = Fixture::new().await;
    let account = Uuid::new_v4();
    let session = Uuid::new_v4();
    let link = Uuid::new_v4();
    let full = AuditEvent {
        actor_account_id: Some(account),
        actor_session_id: Some(session),
        subject: Some(SubjectRef {
            subject_type: "share_link",
            id: link,
        }),
        ip: Some("203.0.113.9".parse::<IpAddr>().expect("ip")),
        detail: AuditDetail::new()
            .with_id("device_id", link)
            .with_code("limiter", "share_password")
            .with_int("attempts", 11),
        ..event(AuditEventKind::AbuseTrip)
    };
    fx.clock.tick(Duration::from_secs(90));

    fx.api
        .run(Scope::Account(account), async |tx| {
            fx.writer.record_in(tx, full.clone()).await
        })
        .await
        .expect("record_in");
    fx.writer
        .record(event(AuditEventKind::SignIn))
        .await
        .expect("record");

    let mut admin = fx.db.admin().await;
    let row: StoredRow =
        sqlx::query_as(
            "SELECT event, actor_account_id, actor_session_id, subject_type, subject_id, host(ip), detail::text, \
             (EXTRACT(EPOCH FROM at) * 1000000)::bigint FROM audit_log WHERE event = 'abuse_trip'",
        )
        .fetch_one(&mut admin)
        .await
        .expect("stored row");
    let bare: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT detail::text, host(ip) FROM audit_log WHERE event = 'sign_in'")
            .fetch_one(&mut admin)
            .await
            .expect("bare row");
    assert_eq!(row.0, "abuse_trip");
    assert_eq!((row.1, row.2), (Some(account), Some(session)));
    assert_eq!((row.3.as_deref(), row.4), (Some("share_link"), Some(link)));
    assert_eq!(row.5.as_deref(), Some("203.0.113.9"));
    let expected_detail =
        format!("{{\"limiter\": \"share_password\", \"attempts\": 11, \"device_id\": \"{link}\"}}");
    assert_eq!(row.6, expected_detail);
    assert_eq!(row.7, START.as_unix_micros() + 90_000_000);
    assert_eq!(bare, (None, None), "empty detail and no address are NULL");
    fx.db.drop_db().await;
}

#[tokio::test]
async fn every_event_kind_is_accepted_by_the_database_check() {
    let fx = Fixture::new().await;

    for kind in AuditEventKind::ALL {
        fx.writer
            .record(event(kind))
            .await
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
    }

    let stored: Vec<String> = sqlx::query_scalar("SELECT event FROM audit_log ORDER BY id")
        .fetch_all(&mut fx.db.admin().await)
        .await
        .expect("events");
    let expected: Vec<&str> = AuditEventKind::ALL
        .iter()
        .map(|kind| kind.as_str())
        .collect();
    assert_eq!(stored, expected);
    fx.db.drop_db().await;
}

#[tokio::test]
async fn detail_of_2049_bytes_is_rejected_before_any_insert_and_2048_is_stored() {
    let fx = Fixture::new().await;
    let at_limit = AuditEvent {
        detail: detail_of(MAX_DETAIL_BYTES),
        ..event(AuditEventKind::AdminAction)
    };
    let over = AuditEvent {
        detail: detail_of(MAX_DETAIL_BYTES + 1),
        ..event(AuditEventKind::AdminAction)
    };

    let rejected_own = fx.writer.record(over.clone()).await;
    let rejected_in = fx
        .api
        .run(Scope::Account(Uuid::nil()), async |tx| {
            fx.writer.record_in(tx, over.clone()).await
        })
        .await;
    fx.writer.record_best_effort(over).await;
    let count_after_rejections = fx.db.admin_count("SELECT count(*) FROM audit_log").await;
    fx.writer
        .record(at_limit)
        .await
        .expect("2048 bytes are stored");

    let expected = AuditError::Rejected(Rejection::DetailTooLarge {
        bytes: MAX_DETAIL_BYTES + 1,
    });
    assert_eq!(rejected_own, Err(expected.clone()));
    assert!(matches!(rejected_in, Err(RunError::Closure(error)) if error == expected));
    assert_eq!(count_after_rejections, 0, "no row for a rejected event");
    assert_eq!(
        fx.db
            .admin_count("SELECT max(octet_length(detail::text))::bigint FROM audit_log")
            .await,
        i64::try_from(MAX_DETAIL_BYTES).expect("fits")
    );
    fx.db.drop_db().await;
}

#[tokio::test]
async fn a_failed_audit_insert_rolls_the_business_change_back() {
    let fx = Fixture::new().await;
    fx.db.admin_exec(BUSINESS_TABLE).await;
    let business_id = Uuid::new_v4();
    let unit = async |fx: &Fixture, id: Uuid| {
        fx.api
            .run(Scope::Account(Uuid::nil()), async |tx| {
                sqlx::query("INSERT INTO business_probe (id) VALUES ($1)")
                    .bind(id)
                    .execute(tx.conn())
                    .await
                    .map_err(|error| AuditError::Store(DbError::from(error)))?;
                fx.writer
                    .record_in(tx, event(AuditEventKind::DeviceApproved))
                    .await
            })
            .await
    };

    unit(&fx, business_id)
        .await
        .expect("both statements commit together");
    let committed = (
        fx.db
            .admin_count("SELECT count(*) FROM business_probe")
            .await,
        fx.db.admin_count("SELECT count(*) FROM audit_log").await,
    );
    fx.db.admin_exec(BLOCK_AUDIT_INSERT).await;
    let failed = unit(&fx, Uuid::new_v4()).await;

    assert_eq!(committed, (1, 1));
    assert!(
        matches!(failed, Err(RunError::Closure(AuditError::Store(_)))),
        "{failed:?}"
    );
    let after = (
        fx.db
            .admin_count("SELECT count(*) FROM business_probe")
            .await,
        fx.db.admin_count("SELECT count(*) FROM audit_log").await,
    );
    assert_eq!(
        after,
        (1, 1),
        "the second business row was rolled back with the failed audit insert"
    );
    fx.db.drop_db().await;
}

#[tokio::test]
async fn an_event_recorded_in_a_transaction_that_rolls_back_is_not_stored() {
    let fx = Fixture::new().await;

    let outcome = fx
        .api
        .run(Scope::Account(Uuid::nil()), async |tx| {
            fx.writer
                .record_in(tx, event(AuditEventKind::ShareCreated))
                .await?;
            Err::<(), AuditError>(AuditError::Store(DbError::Serialization))
        })
        .await;

    assert!(matches!(
        outcome,
        Err(RunError::Closure(AuditError::Store(DbError::Serialization)))
    ));
    assert_eq!(fx.db.admin_count("SELECT count(*) FROM audit_log").await, 0);
    fx.db.drop_db().await;
}

#[tokio::test]
async fn record_returns_store_when_the_insert_fails() {
    let fx = Fixture::new().await;
    fx.db.admin_exec(BLOCK_AUDIT_INSERT).await;

    let outcome = fx.writer.record(event(AuditEventKind::SignIn)).await;

    assert!(matches!(outcome, Err(AuditError::Store(_))), "{outcome:?}");
    assert_eq!(
        fx.writer.stats().write_failures,
        0,
        "only best-effort failures are counted"
    );
    fx.db.drop_db().await;
}

#[tokio::test]
async fn best_effort_failure_returns_normally_logs_one_error_naming_the_kind_and_counts() {
    let fx = Fixture::new().await;
    fx.db.admin_exec(BLOCK_AUDIT_INSERT).await;
    let logs = LogCapture::install();
    let before = fx.writer.stats();

    fx.writer
        .record_best_effort(event(AuditEventKind::ShareAccessed))
        .await;

    let errors: Vec<_> = logs
        .events()
        .into_iter()
        .filter(|captured| captured.level == tracing::Level::ERROR)
        .collect();
    assert_eq!(before.write_failures, 0);
    assert_eq!(fx.writer.stats().write_failures, 1);
    assert_eq!(errors.len(), 1, "{:?}", logs.lines());
    assert_eq!(
        errors[0].fields.get("event").map(String::as_str),
        Some("share_accessed")
    );
    assert!(
        logs.audit_lines().is_empty(),
        "a failed write is not mirrored as recorded"
    );
    fx.db.drop_db().await;
}

#[tokio::test]
async fn best_effort_success_stores_the_row_and_leaves_the_counter_alone() {
    let fx = Fixture::new().await;

    fx.writer
        .record_best_effort(event(AuditEventKind::ScrubSummary))
        .await;
    fx.writer
        .record_best_effort(AuditEvent {
            detail: detail_of(MAX_DETAIL_BYTES + 1),
            ..event(AuditEventKind::ScrubMismatch)
        })
        .await;

    assert_eq!(fx.db.admin_count("SELECT count(*) FROM audit_log").await, 1);
    assert_eq!(
        fx.writer.stats().write_failures,
        1,
        "the rejected event is the one failure"
    );
    fx.db.drop_db().await;
}

#[tokio::test]
async fn three_recorded_events_produce_three_audit_lines_with_ids_only() {
    let fx = Fixture::new().await;
    let logs = LogCapture::install();
    let account = Uuid::new_v4();
    let link = Uuid::new_v4();

    fx.api
        .run(Scope::Account(account), async |tx| {
            fx.writer
                .record_in(
                    tx,
                    AuditEvent {
                        actor_account_id: Some(account),
                        ..event(AuditEventKind::SignIn)
                    },
                )
                .await
        })
        .await
        .expect("record_in");
    fx.writer
        .record(AuditEvent {
            subject: Some(SubjectRef {
                subject_type: "share_link",
                id: link,
            }),
            detail: AuditDetail::new().with_int("downloads", 3),
            ..event(AuditEventKind::ShareRevoked)
        })
        .await
        .expect("record");
    fx.writer
        .record_best_effort(event(AuditEventKind::ShareAccessed))
        .await;

    let lines: Vec<String> = logs
        .lines()
        .into_iter()
        .filter(|line| line.contains("\"stream\":\"audit\""))
        .collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        logs.other_lines().is_empty(),
        "no other line is logged on success"
    );
    let allowed = [
        "event",
        "actor_account_id",
        "actor_session_id",
        "subject_type",
        "subject_id",
        "ip",
        "detail",
        "message",
    ];
    for captured in logs.audit_lines() {
        for (name, value) in &captured.fields {
            assert!(allowed.contains(&name.as_str()), "unexpected field {name}");
            if name.ends_with("_id") {
                assert!(Uuid::parse_str(value).is_ok(), "{name}={value}");
            }
        }
    }
    let kinds: Vec<_> = logs
        .audit_lines()
        .iter()
        .map(|c| c.fields["event"].clone())
        .collect();
    assert_eq!(kinds, ["sign_in", "share_revoked", "share_accessed"]);
    let revoked = &logs.audit_lines()[1];
    assert_eq!(revoked.fields["subject_id"], link.to_string());
    assert_eq!(revoked.fields["detail"], "{\"downloads\": 3}");
    fx.db.drop_db().await;
}

#[tokio::test]
async fn clock_stamps_each_event_when_it_is_written() {
    let fx = Fixture::new().await;

    fx.writer
        .record(event(AuditEventKind::SignIn))
        .await
        .expect("first");
    fx.clock.tick(Duration::from_secs(5));
    fx.writer
        .record(event(AuditEventKind::SignIn))
        .await
        .expect("second");

    let stamps: Vec<i64> = sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM at) * 1000000)::bigint FROM audit_log ORDER BY id",
    )
    .fetch_all(&mut fx.db.admin().await)
    .await
    .expect("stamps");
    assert_eq!(
        stamps,
        [START.as_unix_micros(), fx.clock.now_utc().as_unix_micros()]
    );
    fx.db.drop_db().await;
}
