//! AZ-3875 AC-1 (IT-09-18, test build): the real worker binary follows the API's clock offset. Built only with
//! feature `test-clock`; `clock_poll_release.rs` covers the build without it.
#![cfg(feature = "test-clock")]

mod api_support;
mod process_support;
mod slot_support;
mod support;

use std::time::Duration;

use process_support::{DAY_SECONDS, Process};
use support::{TOKEN, http_get};

const HOUR_SECONDS: u64 = 3_600;

/// Twenty days and a half: a whole-day boundary is 12 hours from now on both sides of every offset used.
fn image_age() -> Duration {
    Duration::from_secs(20 * DAY_SECONDS + DAY_SECONDS / 2)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac1_the_image_age_follows_the_offset_the_api_reports_by_whole_days() {
    // Arrange
    let mut process = Process::start(image_age()).await;
    process
        .wait_for("a report at offset 0", |p| p.status_ages().contains(&20))
        .await;

    // Act
    process.api.set_clock_offset(HOUR_SECONDS);
    let polls_then = process.offset_polls().len();
    process
        .wait_for("two polls after the one-hour advance", |p| {
            p.offset_polls().len() >= polls_then + 2
        })
        .await;
    let after_one_hour = process.status_ages();
    process.api.set_clock_offset(3 * DAY_SECONDS + HOUR_SECONDS);
    process
        .wait_for("a report with the three-day advance", |p| {
            p.status_ages().contains(&23)
        })
        .await;
    let reports_then = process.status_ages().len();
    process
        .wait_for("two more reports", |p| {
            p.status_ages().len() >= reports_then + 2
        })
        .await;
    let ages = process.status_ages();
    let (readyz, _) = http_get(process.worker.addr, "/readyz");

    // Assert
    assert!(
        after_one_hour.iter().all(|age| *age == 20),
        "{after_one_hour:?}"
    );
    assert!(
        ages[reports_then - 1..].iter().all(|age| *age == 23),
        "{ages:?}"
    );
    assert_eq!(readyz, 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac1_the_poll_carries_the_worker_bearer_and_comes_at_most_once_a_second() {
    // Arrange
    let mut process = Process::start(image_age()).await;
    process.api.set_clock_offset(HOUR_SECONDS);

    // Act
    process
        .wait_for("three polls", |p| p.offset_polls().len() >= 3)
        .await;

    // Assert
    let polls = process.offset_polls();
    let bearer = format!("Bearer {TOKEN}");
    assert!(
        polls.iter().all(|poll| poll.method == "GET"
            && poll.header("authorization") == Some(bearer.as_str())
            && poll.header("x-worker-id").is_some()),
        "a poll lacks the worker credentials"
    );
    let gaps: Vec<Duration> = polls.windows(2).map(|w| w[1].at - w[0].at).collect();
    assert!(
        gaps.iter().all(|gap| *gap >= Duration::from_millis(900)),
        "polls closer than 1 s: {gaps:?}"
    );
}
