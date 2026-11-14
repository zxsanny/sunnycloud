//! AZ-3846 AC-1 (IT-08-20 doctor part, AC-63), AC-3 (the media health item) and AC-2 (IT-08-20 requeue and
//! stats): worker presence from the `worker_status` rows the real `record_status` writes, and the operator port.

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    DoctorCheck, DoctorContext, FindingLevel, HealthContributor, HealthStatus, JobCount, JobKind,
    JobStatus, Lane, MediaAdmin, RequeueFilter, WorkerJobs, WorkerStatus,
};
use media_api::{MediaAdminService, MediaHealth, Priority, WorkerJobsFacade};
use media_shared::types as wire;
use sqlx::Connection;

use crate::accept_support::Accept;
use crate::queue_support::{Queue, config};

const INTERVAL: Duration = Duration::from_secs(1);
const WORKER: &str = "gpu-host-4711";

fn health(a: &Accept) -> MediaHealth {
    let mut media = config();
    media.worker_status_interval = INTERVAL;
    MediaHealth::new(a.q.fixture.api.clone(), a.q.clock.clone(), &media)
}

fn level_of(findings: &[contracts::Finding]) -> FindingLevel {
    if findings.iter().any(|f| f.level == FindingLevel::Fail) {
        FindingLevel::Fail
    } else if findings.iter().any(|f| f.level == FindingLevel::Warn) {
        FindingLevel::Warn
    } else {
        FindingLevel::Ok
    }
}

#[tokio::test]
async fn doctor_levels_and_the_health_item_follow_the_worker_status_rows() {
    // Arrange
    let a = Accept::new().await;
    let check = health(&a);
    let jobs = WorkerJobsFacade::new(
        a.q.fixture.api.clone(),
        a.q.clock.clone(),
        a.q.queue.clone(),
        Arc::clone(&a.store),
    );
    let report = |decoder_ok: bool, image_age_days: u32| {
        jobs.record_status(
            WORKER,
            WorkerStatus {
                decoder_ok,
                image_age_days,
            },
        )
    };
    let mut observed = Vec::new();

    // Act
    let mut step = async |case: &'static str| {
        let findings = check.run(&DoctorContext::default()).await;
        let item = check.check().await;
        observed.push((case, level_of(&findings), findings, item));
    };
    step("no row, no jobs").await;
    report(true, 3).await.expect("report");
    step("fresh, age 3").await;
    report(true, 15).await.expect("report");
    step("fresh, age 15").await;
    report(false, 3).await.expect("report");
    step("decoder failing").await;
    report(true, 3).await.expect("report");
    a.q.clock.tick(INTERVAL * 3 + Duration::from_millis(1));
    step("stale row, no jobs").await;
    let job =
        a.q.add(wire::JobKind::Stills, 1, Priority::Interactive)
            .await;
    step("stale row, a job queued").await;
    let mut conn = a.q.fixture.owner_conn().await;
    sqlx::query("DELETE FROM worker_status")
        .execute(&mut conn)
        .await
        .expect("drop the rows");
    conn.close().await.expect("close owner connection");
    step("no row, a job queued").await;
    a.q.queue
        .claim(wire::Lane::Image, "w")
        .await
        .expect("claim")
        .expect("job");
    a.q.queue
        .fail(job, "w", "corrupt", wire::Disposition::Permanent)
        .await
        .expect("fail");
    step("no row, a job dead").await;

    // Assert
    let levels: Vec<(&str, FindingLevel)> = observed
        .iter()
        .map(|(case, level, ..)| (*case, *level))
        .collect();
    assert_eq!(
        levels,
        [
            ("no row, no jobs", FindingLevel::Warn),
            ("fresh, age 3", FindingLevel::Ok),
            ("fresh, age 15", FindingLevel::Warn),
            ("decoder failing", FindingLevel::Fail),
            ("stale row, no jobs", FindingLevel::Warn),
            ("stale row, a job queued", FindingLevel::Fail),
            ("no row, a job queued", FindingLevel::Fail),
            ("no row, a job dead", FindingLevel::Fail),
        ]
    );
    let account = a.q.account.to_string();
    for (case, level, findings, item) in &observed {
        let expected = match level {
            FindingLevel::Ok => HealthStatus::Ok,
            FindingLevel::Warn => HealthStatus::Degraded,
            FindingLevel::Fail => HealthStatus::Down,
        };
        assert_eq!(item.status, expected, "{case}: {item:?}");
        let worst = findings
            .iter()
            .find(|finding| finding.level == *level)
            .expect("the finding of the level");
        assert!(item.detail.contains(&worst.message), "{case}: {item:?}");
        for text in std::iter::once(item.detail.as_str())
            .chain(findings.iter().map(|finding| finding.message.as_str()))
        {
            assert!(
                !text.contains(WORKER) && !text.contains(&account),
                "{case}: {text}"
            );
        }
    }
    assert!(observed[0].2[0].message.contains("no media worker"));
    assert!(observed[6].3.detail.contains("queue depth 1; dead jobs 0"));
    assert!(observed[7].3.detail.contains("queue depth 0; dead jobs 1"));
    drop((check, jobs));
    a.finish().await;
}

/// Leases the job of `kind` just enqueued and fails it for good.
async fn dead(q: &Queue, account: uuid::Uuid, revision: uuid::Uuid, kind: wire::JobKind, n: u32) {
    q.enqueue(q.spec_of(account, revision, kind, n), Priority::Interactive)
        .await;
    let job = q
        .queue
        .claim(kind.lane(), "w")
        .await
        .expect("claim")
        .expect("job");
    q.queue
        .fail(job.job_id, "w", "corrupt", wire::Disposition::Permanent)
        .await
        .expect("fail");
}

async fn direct_counts(q: &Queue) -> Vec<(String, String, i64)> {
    let mut conn = q.fixture.owner_conn().await;
    let rows = sqlx::query_as(
        "SELECT lane, status, count(*) FROM jobs GROUP BY lane, status ORDER BY lane, status",
    )
    .fetch_all(&mut conn)
    .await
    .expect("direct counts");
    conn.close().await.expect("close owner connection");
    rows
}

fn names(count: &JobCount) -> (String, String, i64) {
    let lane = match count.lane {
        Lane::Image => "image",
        Lane::Video => "video",
    };
    let status = match count.status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Succeeded => "succeeded",
        JobStatus::Dead => "dead",
    };
    (
        lane.to_owned(),
        status.to_owned(),
        i64::try_from(count.count).expect("count"),
    )
}

#[tokio::test]
async fn requeue_dead_by_account_then_by_kind_and_stats_equal_a_direct_count() {
    // Arrange
    let q = Queue::new().await;
    let (other, other_revision) = q.second_account().await;
    for (account, revision, kind, n) in [
        (q.account, q.revision, wire::JobKind::Stills, 1),
        (q.account, q.revision, wire::JobKind::Transcode, 2),
        (other, other_revision, wire::JobKind::Stills, 3),
        (other, other_revision, wire::JobKind::Transcode, 4),
    ] {
        dead(&q, account, revision, kind, n).await;
    }
    q.add(wire::JobKind::Poster, 5, Priority::Interactive).await;
    q.queue
        .claim(wire::Lane::Video, "w")
        .await
        .expect("claim")
        .expect("job");
    let admin = MediaAdminService::new(q.fixture.api.clone(), q.queue.clone(), q.metrics.clone());

    // Act
    let by_account = admin
        .requeue_dead(RequeueFilter {
            account: Some(q.account),
            kind: None,
        })
        .await
        .expect("by account");
    let by_kind = admin
        .requeue_dead(RequeueFilter {
            account: None,
            kind: Some(JobKind::Transcode),
        })
        .await
        .expect("by kind");
    let stats = admin.stats().await.expect("stats");

    // Assert
    assert_eq!((by_account, by_kind), (2, 1));
    let mut conn = q.fixture.owner_conn().await;
    let requeued: Vec<(i32, Option<String>)> = sqlx::query_as(
        "SELECT attempts::int, leased_by FROM jobs WHERE status = 'queued' ORDER BY id",
    )
    .fetch_all(&mut conn)
    .await
    .expect("requeued rows");
    let still_dead: Vec<(uuid::Uuid, String)> =
        sqlx::query_as("SELECT account_id, kind FROM jobs WHERE status = 'dead'")
            .fetch_all(&mut conn)
            .await
            .expect("dead rows");
    conn.close().await.expect("close owner connection");
    assert_eq!(requeued, vec![(0, None); 3]);
    assert_eq!(still_dead, vec![(other, "stills".to_owned())]);
    let counted: Vec<(String, String, i64)> = stats.counts.iter().map(names).collect();
    assert_eq!(counted, direct_counts(&q).await);
    drop(admin);
    q.finish().await;
}
