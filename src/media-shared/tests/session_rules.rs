//! AC-6: the session state check (one job in flight, `Busy`, `cancel`, repeated `hello`).

mod common;

use common::*;
use media_shared::decoder_protocol::{
    Admission, ProtocolViolation, Request, Session, ViolationKind,
};
use media_shared::policy::Plan;

fn probe() -> Request {
    Request::Probe {
        job_dir: "/s/j".to_owned(),
    }
}

fn render_still() -> Request {
    requests()
        .into_iter()
        .find(|r| matches!(r, Request::RenderStill { .. }))
        .expect("sample")
}

fn transcode() -> Request {
    Request::Transcode {
        job_dir: "/s/j".to_owned(),
        plan: Plan::Remux,
        limits: limits(),
    }
}

fn self_check() -> Request {
    Request::SelfCheck {
        forbidden_targets: vec!["api:8080".to_owned()],
    }
}

fn running(first: &Request) -> Session {
    let mut session = Session::new();
    assert_eq!(session.admit(first), Ok(Admission::Accepted));
    assert!(session.job_in_flight());
    session
}

const OUT_OF_SEQUENCE: ProtocolViolation = ProtocolViolation(ViolationKind::OutOfSequence);

#[test]
fn every_kind_of_job_request_makes_the_next_job_request_busy() {
    for first in [probe(), render_still(), transcode()] {
        for second in [probe(), render_still(), transcode()] {
            let mut session = running(&first);

            let verdict = session.admit(&second);

            assert_eq!(verdict, Ok(Admission::Busy), "{first:?} then {second:?}");
            assert!(session.job_in_flight(), "the running job continues");
        }
    }
}

#[test]
fn cancel_while_a_job_runs_is_accepted() {
    let mut session = running(&render_still());

    assert_eq!(session.admit(&Request::Cancel), Ok(Admission::Accepted));

    session.job_finished();
    assert!(!session.job_in_flight());
}

#[test]
fn hello_is_accepted_each_time_on_an_idle_session() {
    let mut session = Session::new();

    for protocol in [1, 1, 99, 1] {
        assert_eq!(
            session.admit(&Request::Hello { protocol }),
            Ok(Admission::Accepted)
        );
    }
    assert!(!session.job_in_flight());
}

#[test]
fn self_check_is_accepted_on_an_idle_session() {
    let mut session = Session::new();

    assert_eq!(session.admit(&self_check()), Ok(Admission::Accepted));
    assert!(!session.job_in_flight());
}

#[test]
fn hello_or_self_check_while_a_job_runs_is_out_of_sequence() {
    for request in [Request::Hello { protocol: 1 }, self_check()] {
        let mut session = running(&transcode());

        let verdict = session.admit(&request);

        assert_eq!(verdict, Err(OUT_OF_SEQUENCE), "{request:?}");
        assert!(session.job_in_flight());
    }
}

#[test]
fn cancel_on_an_idle_session_is_a_no_op() {
    let mut session = Session::new();

    assert_eq!(session.admit(&Request::Cancel), Ok(Admission::Accepted));

    assert!(!session.job_in_flight());
}

#[test]
fn a_new_job_is_accepted_after_the_previous_one_finished() {
    let mut session = running(&probe());
    session.job_finished();

    assert_eq!(session.admit(&render_still()), Ok(Admission::Accepted));
    assert!(session.job_in_flight());
}

#[test]
fn a_busy_answer_does_not_start_a_job() {
    let mut session = running(&probe());
    assert_eq!(session.admit(&transcode()), Ok(Admission::Busy));

    session.job_finished();

    assert!(!session.job_in_flight());
    assert_eq!(
        session.admit(&Request::Hello { protocol: 1 }),
        Ok(Admission::Accepted)
    );
}
