//! AC-3: every call ends at the request timeout, and a late answer reaches nobody.

mod common;

use std::time::{Duration, Instant};

use sunnycloud_settings::daemon_client::{ClientError, ConnectionState};
use sunnycloud_settings_testkit::script::{Answer, Script};
use tracing::Level;

use common::{Captured, session_with};

const TIMEOUT: Duration = Duration::from_millis(300);
/// The late answer of the spec's "6 s against a 5 s timeout", at the same ratio.
const LATE_MS: u64 = 360;

#[tokio::test]
async fn a_silent_call_and_a_late_call_both_time_out_and_the_late_answer_is_dropped() {
    let captured = Captured::default();
    let _guard = captured.install();
    let script = Script::default()
        .with_answers("get_settings", vec![Answer::Silent])
        .with_answers(
            "list_pairs",
            vec![Answer::Delay {
                ms: LATE_MS,
                then: Box::new(Answer::Default),
            }],
        );
    let s = session_with(script, |c| c.request_timeout = TIMEOUT).await;
    let started = Instant::now();

    let silent = async {
        let result = s.client.get_settings().await;
        (result, started.elapsed())
    };
    let late = async {
        let result = s.client.list_pairs().await;
        (result, started.elapsed())
    };
    let ((silent_result, silent_at), (late_result, late_at)) = tokio::join!(silent, late);

    assert_eq!(silent_result, Err(ClientError::Timeout));
    assert_eq!(late_result, Err(ClientError::Timeout));
    for at in [silent_at, late_at] {
        assert!(
            at >= TIMEOUT && at < TIMEOUT * 3,
            "timed out at {at:?}, expected about {TIMEOUT:?}"
        );
    }

    // Let the late answer arrive, then prove the connection and the id matching are intact.
    tokio::time::sleep(Duration::from_millis(LATE_MS)).await;
    let status = s.client.status().await.expect("a later call is answered");
    assert_eq!(status.skipped_count, 0);
    assert_eq!(*s.client.connection().borrow(), ConnectionState::Connected);
    assert_eq!(
        captured.count(Level::ERROR, "protocol violation"),
        0,
        "a late answer is dropped quietly: {:?}",
        captured.events()
    );
}

#[test]
fn the_production_timeout_is_five_seconds() {
    assert_eq!(
        sunnycloud_settings::daemon_client::REQUEST_TIMEOUT,
        Duration::from_secs(5)
    );
}
