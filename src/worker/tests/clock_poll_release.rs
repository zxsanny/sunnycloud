//! AZ-3875 AC-2 (IT-09-18, FT-N-35): a build without feature `test-clock` sends no offset poll and contains
//! no offset clock. Run with the default features of this crate alone (`cargo test -p sunnycloud-worker`).
#![cfg(not(feature = "test-clock"))]

mod api_support;
mod process_support;
mod slot_support;
mod support;

use std::fs;
use std::time::Duration;

use process_support::{DAY_SECONDS, OFFSET_PATH, Process};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ac2_the_release_worker_never_requests_the_offset_route() {
    // Arrange
    let mut process = Process::start(Duration::from_secs(20 * DAY_SECONDS)).await;
    process.api.set_clock_offset(3 * DAY_SECONDS);

    // Act
    process
        .wait_for("six status reports", |p| p.status_ages().len() >= 6)
        .await;
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Assert
    let ages = process.status_ages();
    let polls = process
        .collect()
        .iter()
        .filter(|r| r.path == OFFSET_PATH)
        .count();
    assert_eq!(polls, 0, "the release worker polled the offset route");
    assert!(ages.iter().all(|age| *age == 20), "{ages:?}");
}

#[test]
fn ac2_the_release_binary_has_no_offset_clock_and_no_offset_route() {
    // Arrange
    let path = env!("CARGO_BIN_EXE_sunnycloud-worker");

    // Act
    let binary = fs::read(path).expect("the worker binary is readable");

    // Assert: markers of the worker's own feature-gated code. `OffsetClock` itself is not a usable needle here:
    // `cargo test --workspace` unifies features, and `sc_clock`'s dev-dependency on itself with `test-clock`
    // puts the type's name into the debug info of every test-profile binary. The release image check asserts it.
    for needle in ["follow_api_offset", "test/clock-offset"] {
        assert!(
            !binary
                .windows(needle.len())
                .any(|window| window == needle.as_bytes()),
            "the release binary contains `{needle}`"
        );
    }
}
