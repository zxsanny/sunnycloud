//! IT-08-1 (AZ-3841 AC-1, AC-5): the planning matrix through the effect, `on_begin` and `on_batch` add nothing,
//! and the statements one entry costs.

use contracts::{Begin, CommitEffect, EntryOutcome, EntryResult, PublishOutcome, PublishedEntry};
use sc_db::Scope;
use sqlx::{AssertSqlSafe, Connection};

use crate::effect_support::{Content, JPEG, Job, MP4, configured, current, entry, jobs, run};
use crate::queue_support::Queue;
use crate::statements::StatementLog;

#[tokio::test]
async fn each_type_gets_the_jobs_of_the_matrix_in_its_own_transaction() {
    // Arrange
    let q = Queue::new().await;
    let effect = configured(&q);
    let stills = vec![Job::new("stills", "image", 0)];
    let video = vec![
        Job::new("poster", "video", 0),
        Job::new("transcode", "video", 0),
    ];
    let cases: Vec<(&str, Content, bool, Vec<Job>)> = vec![
        ("jpeg", Content::new(1, JPEG), false, stills.clone()),
        (
            "png",
            Content::new(2, Some("image/png")),
            false,
            stills.clone(),
        ),
        (
            "webp",
            Content::new(3, Some("image/webp")),
            false,
            stills.clone(),
        ),
        (
            "gif",
            Content::new(4, Some("image/gif")),
            false,
            stills.clone(),
        ),
        (
            "heic",
            Content::new(5, Some("image/heic")),
            false,
            stills.clone(),
        ),
        ("mp4", Content::new(6, MP4), false, video.clone()),
        (
            "mov",
            Content::new(7, Some("video/quicktime")),
            false,
            video.clone(),
        ),
        (
            "mkv",
            Content::new(8, Some("video/x-matroska")),
            false,
            video.clone(),
        ),
        ("svg", Content::new(9, Some("image/svg+xml")), false, vec![]),
        (
            "octet",
            Content::new(10, Some("application/octet-stream")),
            false,
            vec![],
        ),
        (
            "zero-byte",
            Content {
                size: 0,
                ..Content::new(11, JPEG)
            },
            false,
            vec![],
        ),
        ("vault jpeg", Content::new(12, JPEG), true, vec![]),
    ];
    let mut entries = Vec::new();
    for (name, content, vault, expected) in cases {
        let published = PublishedEntry {
            vault,
            ..entry(&q, &content).await
        };
        entries.push((name, published, expected));
    }

    // Act
    for (_, published, _) in &entries {
        run(&q, &effect, q.account, published)
            .await
            .expect("the effect runs");
    }

    // Assert
    for (name, published, expected) in &entries {
        assert_eq!(&jobs(&q, &published.content_hash).await, expected, "{name}");
    }
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 11);
    drop(effect);
    q.finish().await;
}

#[tokio::test]
async fn on_begin_proceeds_and_on_batch_adds_no_job() {
    // Arrange
    let q = Queue::new().await;
    let effect = current(&q);
    let published = entry(&q, &Content::new(1, JPEG)).await;
    let outcomes = vec![EntryOutcome {
        index: 0,
        result: EntryResult::Published {
            outcome: PublishOutcome::Created,
            node_id: published.node_id,
            revision_id: published.revision_id,
            conflict_copy: None,
        },
    }];

    // Act
    let begin = q
        .fixture
        .api
        .run(Scope::Account(q.account), async |tx| {
            let begin = effect.on_begin(tx).await?;
            effect.on_batch(tx, &outcomes).await?;
            Ok::<_, contracts::EffectError>(begin)
        })
        .await
        .expect("the hooks run");

    // Assert
    assert_eq!(begin, Begin::Proceed);
    assert_eq!(q.count("SELECT count(*) FROM jobs").await, 0);
    drop(effect);
    q.finish().await;
}

/// The statements `on_entry` ran inside the transaction, captured from sqlx's statement events.
async fn statements_of(q: &Queue, published: &PublishedEntry) -> Vec<String> {
    let effect = current(q);
    let log = StatementLog::install();
    q.fixture
        .api
        .run(Scope::Account(q.account), async |tx| {
            log.take();
            effect.on_entry(tx, published).await?;
            Ok::<_, contracts::EffectError>(log.take())
        })
        .await
        .expect("the effect runs")
}

/// The plan of a captured probe with sequential scans disabled: it names the index when the probe can use one.
/// The parameters are those of the entry (account, hash, params version 1) or the limit 6.
async fn plan_of(q: &Queue, sql: &str, published: &PublishedEntry) -> String {
    let mut conn = q.fixture.owner_conn().await;
    sqlx::query("SET enable_seqscan = off")
        .execute(&mut conn)
        .await
        .expect("disable sequential scans");
    let explain = sqlx::query_scalar(AssertSqlSafe(format!("EXPLAIN {sql}"))).bind(q.account);
    let explain = if sql.contains("FROM derived") {
        explain
            .bind(published.content_hash.as_bytes().as_slice())
            .bind(1_i64)
    } else {
        explain.bind(6_i64)
    };
    let lines: Vec<String> = explain.fetch_all(&mut conn).await.expect("explain probe");
    conn.close().await.expect("close owner connection");
    lines.join("\n")
}

fn shape(statements: &[String]) -> Vec<&'static str> {
    statements
        .iter()
        .map(|sql| {
            let sql = sql.trim();
            if sql.starts_with("SELECT kind FROM derived") {
                "probe derived"
            } else if sql.starts_with("SELECT count(*) FROM (SELECT 1 FROM jobs") {
                "probe queued"
            } else if sql.starts_with("INSERT INTO jobs") {
                "insert job"
            } else {
                "other"
            }
        })
        .collect()
}

#[tokio::test]
async fn a_photo_entry_costs_two_indexed_probes_and_one_insert() {
    // Arrange
    let q = Queue::new().await;
    let published = entry(&q, &Content::new(1, JPEG)).await;

    // Act
    let statements = statements_of(&q, &published).await;

    // Assert
    assert_eq!(
        shape(&statements),
        ["probe derived", "probe queued", "insert job"],
        "{statements:#?}"
    );
    let derived_plan = plan_of(&q, &statements[0], &published).await;
    let queued_plan = plan_of(&q, &statements[1], &published).await;
    assert!(derived_plan.contains("derived_pkey"), "{derived_plan}");
    assert!(queued_plan.contains("jobs_queued_idx"), "{queued_plan}");
    q.finish().await;
}

#[tokio::test]
async fn a_video_entry_stays_within_two_probes_and_one_insert_per_job() {
    // Arrange
    let q = Queue::new().await;
    let published = entry(&q, &Content::new(1, MP4)).await;

    // Act
    let statements = statements_of(&q, &published).await;

    // Assert
    assert_eq!(
        shape(&statements),
        ["probe derived", "probe queued", "insert job", "insert job"],
        "{statements:#?}"
    );
    q.finish().await;
}

#[tokio::test]
async fn an_entry_without_planned_jobs_runs_no_statement() {
    // Arrange
    let q = Queue::new().await;
    let published = entry(&q, &Content::new(1, Some("image/svg+xml"))).await;

    // Act
    let statements = statements_of(&q, &published).await;

    // Assert
    assert!(statements.is_empty(), "{statements:#?}");
    q.finish().await;
}
