use hyper::StatusCode;

use super::support::{
    GRACE, Rig, SHORT_JOB, Session, Wire, claim_request, input_request, short_body_len,
    spawn_exchange,
};
use crate::harness::clients::{HalfRequest, refuses_connections};

const HELD_PER_WIRE: usize = 10;
const DOWNLOADS: usize = 3;

#[tokio::test]
async fn the_signal_turns_readiness_off_stops_accepting_then_answers_the_held_requests_and_closes()
{
    // Arrange
    let mut rig = Rig::start().await;
    let readiness = HalfRequest::open(rig.ops, "/readyz").await;
    let mut held = Vec::new();
    for wire in [Wire::H1, Wire::H2] {
        for _ in 0..HELD_PER_WIRE {
            held.push(spawn_exchange(rig.internal, wire, claim_request()));
        }
    }
    let downloads: Vec<_> = [Wire::H1, Wire::H1, Wire::H2]
        .into_iter()
        .map(|wire| spawn_exchange(rig.internal, wire, input_request(SHORT_JOB)))
        .collect();
    rig.jobs.wait_held(2 * HELD_PER_WIRE).await;
    rig.jobs.wait_downloads(DOWNLOADS).await;

    // Act
    let signalled_at = rig.send_signal();
    rig.jobs.wait_woken(2 * HELD_PER_WIRE).await;
    let (readiness_status, readiness_body) = readiness.finish().await;
    let refused = [
        refuses_connections(rig.public).await,
        refuses_connections(rig.internal).await,
        refuses_connections(rig.ops).await,
    ];
    let answered_before_release = held.iter().filter(|poll| poll.is_finished()).count();
    rig.jobs.release_claims();
    let mut answers = Vec::new();
    for poll in held {
        answers.push(poll.await.expect("held request task"));
    }
    let mut transfers = Vec::new();
    for download in downloads {
        transfers.push(download.await.expect("download task"));
    }
    let outcome = rig.finished().await;
    let returned_after = signalled_at.elapsed();

    // Assert
    assert!(
        returned_after < GRACE,
        "every request finished inside the grace, so run did not wait for it: {returned_after:?}"
    );
    assert_eq!(readiness_status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(readiness_body.contains("shutting down"));
    assert_eq!(refused, [true; 3], "no listener accepts once woken");
    assert_eq!(
        answered_before_release, 0,
        "held requests wait for the wake"
    );
    for answer in &answers {
        assert_eq!(answer.status, StatusCode::NO_CONTENT);
        assert!(answer.body.is_ok(), "the answer ends cleanly");
        assert!(answer.connection_closed, "the connection closes after it");
    }
    for transfer in &transfers {
        assert_eq!(transfer.status, StatusCode::OK);
        assert_eq!(
            transfer
                .body
                .as_ref()
                .expect("a short download completes")
                .len(),
            short_body_len()
        );
        assert!(transfer.connection_closed);
    }
    outcome.expect("run returns Ok");
}

#[tokio::test]
async fn an_idle_connection_is_closed_at_once_and_run_does_not_wait_for_the_grace() {
    // Arrange
    let mut rig = Rig::start().await;
    let idle = Session::open(rig.internal, Wire::H1).await;

    // Act
    let signalled_at = rig.send_signal();
    let outcome = rig.finished().await;
    let returned_after = signalled_at.elapsed();
    drop(idle);

    // Assert
    outcome.expect("run returns Ok");
    assert!(returned_after < GRACE, "run took {returned_after:?}");
}
