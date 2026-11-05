//! AC-5: retention through the `SECURITY DEFINER` purge function, called as the API role `sunnycloud`.

mod support;

use std::sync::Arc;
use std::time::Duration;

use sc_audit::{AuditError, AuditEventKind, AuditPurge, AuditWriter};
use sc_clock::{ManualClock, Timestamp};
use support::{Fixture, START, event};

const DAY: Duration = Duration::from_secs(86_400);
const RETENTION: Duration = Duration::from_secs(365 * 86_400);

fn days_before_start(days: u64) -> Timestamp {
    START
        .checked_sub(DAY * u32::try_from(days).expect("small"))
        .expect("in range")
}

/// Records `count` events stamped `days` before [`START`].
async fn seed(fx: &Fixture, days: u64, count: usize) {
    let writer = AuditWriter::new(
        fx.api.clone(),
        Arc::new(ManualClock::new(days_before_start(days))),
    );
    for _ in 0..count {
        writer
            .record(event(AuditEventKind::SignIn))
            .await
            .expect("seed row");
    }
}

async fn remaining(fx: &Fixture) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT (($1::bigint - EXTRACT(EPOCH FROM at) * 1000000) / 86400000000)::bigint \
         FROM audit_log ORDER BY id",
    )
    .bind(START.as_unix_micros())
    .fetch_all(&mut fx.db.admin().await)
    .await
    .expect("remaining rows")
}

#[tokio::test]
async fn purge_deletes_only_rows_older_than_the_cutoff_and_returns_the_count() {
    let fx = Fixture::new().await;
    seed(&fx, 400, 2).await;
    seed(&fx, 366, 1).await;
    seed(&fx, 365, 1).await; // exactly at the cutoff: not older
    seed(&fx, 300, 2).await;
    seed(&fx, 1, 1).await;
    let purge = AuditPurge::new(fx.api.clone(), RETENTION);
    let cutoff = days_before_start(365);

    let deleted = purge.purge_older_than(cutoff).await.expect("purge");
    let again = purge.purge_older_than(cutoff).await.expect("purge again");

    assert_eq!(deleted, 3);
    assert_eq!(again, 0);
    assert_eq!(remaining(&fx).await, [365, 300, 300, 1]);
    fx.db.drop_db().await;
}

#[tokio::test]
async fn retention_zero_keeps_everything_and_runs_no_statement() {
    let fx = Fixture::new().await;
    seed(&fx, 4000, 2).await;
    seed(&fx, 1, 1).await;
    // With the function gone, a purge that ran would fail; with retention 0 it must not run at all.
    fx.db
        .admin_exec("DROP FUNCTION audit_log_purge(timestamptz)")
        .await;
    let keep = AuditPurge::new(fx.api.clone(), Duration::ZERO);
    let run = AuditPurge::new(fx.api.clone(), RETENTION);

    let kept = keep.purge_older_than(START).await;
    let ran = run.purge_older_than(START).await;

    assert_eq!(kept, Ok(0));
    assert!(matches!(ran, Err(AuditError::Store(_))), "{ran:?}");
    assert_eq!(remaining(&fx).await.len(), 3);
    fx.db.drop_db().await;
}
