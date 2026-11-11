use hyper::StatusCode;
use std::time::Duration;

use super::support::{
    GRACE, LONG_JOB, Rig, SHORT_JOB, Wire, input_request, short_body_len, spawn_exchange,
};

/// `run` must return this soon after the grace ends (AC-3).
const RETURN_SLACK: Duration = Duration::from_secs(1);

/// A 1 s and a 5 s download are running when the signal arrives, with a grace of 2 s.
async fn short_and_long(wire: Wire) {
    // Arrange
    let mut rig = Rig::start().await;
    let short = spawn_exchange(rig.internal, wire, input_request(SHORT_JOB));
    let long = spawn_exchange(rig.internal, wire, input_request(LONG_JOB));
    rig.jobs.wait_downloads(2).await;

    // Act
    let signalled_at = rig.send_signal();
    let short = short.await.expect("short download task");
    let long = long.await.expect("long download task");
    let outcome = rig.finished().await;
    let returned_after = signalled_at.elapsed();

    // Assert
    assert_eq!(short.status, StatusCode::OK);
    assert_eq!(
        short.body.expect("the short download completes").len(),
        short_body_len()
    );
    assert_eq!(
        long.status,
        StatusCode::OK,
        "the head went out before the cut"
    );
    let failure = long
        .body
        .expect_err("a download cut at the end of the grace never ends as a clean short body");
    eprintln!("the {wire:?} long download ended with: {failure:?}");
    let cut_after = long.body_ended_at.duration_since(signalled_at);
    assert!(
        (GRACE..GRACE + RETURN_SLACK).contains(&cut_after),
        "the long download ended {cut_after:?} after the signal"
    );
    outcome.expect("run returns Ok");
    assert!(
        returned_after < GRACE + RETURN_SLACK,
        "run returned {returned_after:?} after the signal"
    );
}

#[tokio::test]
async fn over_http1_a_short_download_finishes_and_a_long_one_is_cut_at_the_end_of_the_grace() {
    short_and_long(Wire::H1).await;
}

#[tokio::test]
async fn over_http2_a_short_download_finishes_and_a_long_one_is_reset_at_the_end_of_the_grace() {
    short_and_long(Wire::H2).await;
}
