//! AZ-3805: `admin media-requeue` and `admin media-stats` through the real handler with a recording `MediaAdmin`.

mod support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use contracts::{
    Fault, JobCount, JobKind, JobStatus, Lane, MediaAdmin, MediaAdminError, MediaStats,
    RequeueFilter,
};
use serde_json::Value;
use support::{Captured, Harness, locked};
use uuid::Uuid;

#[derive(Default)]
struct Recorder {
    filters: Mutex<Vec<RequeueFilter>>,
    stats_calls: Mutex<u32>,
    stats: MediaStats,
    requeued: u64,
}

#[async_trait]
impl MediaAdmin for Recorder {
    async fn requeue_dead(&self, filter: RequeueFilter) -> Result<u64, MediaAdminError> {
        locked(&self.filters).push(filter);
        Ok(self.requeued)
    }

    async fn stats(&self) -> Result<MediaStats, MediaAdminError> {
        *locked(&self.stats_calls) += 1;
        Ok(self.stats.clone())
    }
}

struct Failing;

#[async_trait]
impl MediaAdmin for Failing {
    async fn requeue_dead(&self, _: RequeueFilter) -> Result<u64, MediaAdminError> {
        Err(MediaAdminError::Unexpected(Fault::from("queue table gone")))
    }

    async fn stats(&self) -> Result<MediaStats, MediaAdminError> {
        Err(MediaAdminError::Unexpected(Fault::from("queue table gone")))
    }
}

fn run(admin: Arc<dyn MediaAdmin>, args: &[&str]) -> Captured {
    Harness::new()
        .real_handler()
        .service::<dyn MediaAdmin, _, _>(move |_| {
            let admin = Arc::clone(&admin);
            async move { Ok(admin) }
        })
        .run(args)
}

fn recorder(requeued: u64, stats: MediaStats) -> Arc<Recorder> {
    Arc::new(Recorder {
        requeued,
        stats,
        ..Recorder::default()
    })
}

#[test]
fn requeue_with_each_filter_makes_one_call_with_exactly_that_filter_and_prints_the_count() {
    let account = Uuid::new_v4();
    let account_text = account.to_string();
    let cases: [(&[&str], RequeueFilter); 3] = [
        (
            &["admin", "media-requeue", "--account", &account_text],
            RequeueFilter {
                account: Some(account),
                kind: None,
            },
        ),
        (
            &["admin", "media-requeue", "--kind", "transcode"],
            RequeueFilter {
                account: None,
                kind: Some(JobKind::Transcode),
            },
        ),
        (&["admin", "media-requeue"], RequeueFilter::default()),
    ];
    for (args, expected) in cases {
        // Arrange
        let admin = recorder(7, MediaStats::default());

        // Act
        let out = run(admin.clone(), args);

        // Assert
        assert_eq!(out.exit, 0, "{args:?}: {}", out.stderr);
        assert_eq!(*locked(&admin.filters), vec![expected], "{args:?}");
        assert_eq!(out.stdout, "requeued: 7\n", "{args:?}");
    }
}

#[test]
fn an_invalid_kind_or_account_exits_3_with_no_call() {
    for args in [
        &["admin", "media-requeue", "--kind", "banana"][..],
        &["admin", "media-requeue", "--account", "not-an-id"][..],
    ] {
        // Arrange
        let admin = recorder(1, MediaStats::default());

        // Act
        let out = run(admin.clone(), args);

        // Assert
        assert_eq!(out.exit, 3, "{args:?}");
        assert!(locked(&admin.filters).is_empty(), "{args:?}");
        assert_eq!(out.stdout, "", "{args:?}");
    }
}

fn depths() -> MediaStats {
    let count = |lane, status, count| JobCount {
        lane,
        status,
        count,
    };
    MediaStats {
        counts: vec![
            count(Lane::Image, JobStatus::Queued, 5),
            count(Lane::Image, JobStatus::Dead, 2),
            count(Lane::Video, JobStatus::Running, 1),
            count(Lane::Video, JobStatus::Succeeded, 40),
        ],
    }
}

#[test]
fn stats_print_every_lane_and_status_with_the_same_facts_in_text_and_json() {
    // Arrange
    let admin = recorder(0, depths());

    // Act
    let text = run(admin.clone(), &["admin", "media-stats"]);
    let json = run(admin.clone(), &["--json", "admin", "media-stats"]);

    // Assert
    assert_eq!(
        (text.exit, json.exit),
        (0, 0),
        "{}{}",
        text.stderr,
        json.stderr
    );
    assert_eq!(*locked(&admin.stats_calls), 2);
    let data: Value =
        serde_json::from_str::<Value>(&json.stdout).expect("one JSON object")["data"].clone();
    let from_text: Vec<(String, u64)> = text
        .stdout
        .lines()
        .map(|line| {
            let (key, value) = line.split_once(": ").expect("key: value");
            (key.to_owned(), value.parse().expect("a count"))
        })
        .collect();
    assert_eq!(from_text.len(), 8);
    for (key, count) in &from_text {
        assert_eq!(data[key], *count, "{key}");
    }
    let find = |key: &str| {
        from_text
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, count)| *count)
    };
    assert_eq!(
        (
            find("image_queued"),
            find("image_dead"),
            find("video_succeeded"),
            find("video_queued")
        ),
        (Some(5), Some(2), Some(40), Some(0))
    );
}

#[test]
fn a_failing_owner_exits_2_with_the_internal_code() {
    // Act
    let out = run(Arc::new(Failing), &["--json", "admin", "media-stats"]);

    // Assert
    assert_eq!(out.exit, 2, "{}", out.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&out.stdout).expect("envelope")["code"],
        "internal"
    );
}
