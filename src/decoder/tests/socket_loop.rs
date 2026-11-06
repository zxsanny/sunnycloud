//! Socket loop with a scripted job: AC-3, AC-4, AC-5, AC-7 and the session rules around them.
mod common;

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use common::{Fixture, MIB, Received, Scripted, ceilings, greedy_limits, hello, render_still};
use decoder_runner::{Exit, JobSpec, LimitClass, RunnerError};
use media_shared::decoder_protocol::{DecodeError, Event, ImageResult, Reply, Request, Response};
use media_shared::policy::Plan;

const SECOND: Duration = Duration::from_secs(1);

fn wait_until(what: &str, limit: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn image_result() -> Response {
    Response::ImageResult(ImageResult {
        width: 1,
        height: 1,
        bytes: 1,
    })
}

#[test]
fn ac3_second_job_request_is_busy_and_the_first_job_is_unaffected() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(600)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();
    client.send(render_still());
    wait_until("the job to start", SECOND, || {
        fixture.handler.running.load(Ordering::SeqCst) == 1
    });

    client.send(render_still());
    let second = client.expect_response(SECOND);
    let first = client.expect_response(2 * SECOND);

    assert_eq!(second, Response::Error(DecodeError::Busy));
    assert_eq!(first, image_result());
    assert_eq!(fixture.handler.jobs.lock().expect("jobs").len(), 1);
}

#[test]
fn hello_while_a_job_runs_is_a_violation_that_ends_the_runner() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_secs(5)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();
    client.send(render_still());
    wait_until("the job to start", SECOND, || {
        fixture.handler.running.load(Ordering::SeqCst) == 1
    });

    client.send(hello());
    let reply = client.expect_response(SECOND);

    assert_eq!(reply, Response::Error(DecodeError::ProtocolViolation));
    assert_eq!(fixture.outcome().expect("serve"), Exit::ProtocolViolation);
}

#[test]
fn ac4_alive_every_interval_while_busy_progress_per_line_and_nothing_idle() {
    let interval = Duration::from_millis(200);
    let handler = Scripted::new(Duration::from_millis(1500)).lines(3);
    let fixture = Fixture::start(handler, interval);
    let mut client = fixture.connect();
    client.send(render_still());

    let mut alive_at = Vec::new();
    let mut progress = Vec::new();
    let response = loop {
        match client.recv(2 * SECOND) {
            Received::Reply(Reply::Event(Event::Alive)) => alive_at.push(Instant::now()),
            Received::Reply(Reply::Event(Event::Progress { out_time_ms, speed })) => {
                progress.push((out_time_ms, speed));
            }
            Received::Reply(Reply::Response(response)) => break response,
            other => panic!("unexpected {other:?}"),
        }
    };

    assert_eq!(response, image_result());
    assert_eq!(progress, [(1000, 1.5), (2000, 1.5), (3000, 1.5)]);
    assert!(
        (6..=8).contains(&alive_at.len()),
        "1.5 s at 200 ms gives about 7 events, got {}",
        alive_at.len()
    );
    for pair in alive_at.windows(2) {
        let gap = pair[1] - pair[0];
        assert!(
            gap >= Duration::from_millis(120) && gap <= Duration::from_millis(320),
            "Alive gap {gap:?}"
        );
    }
    assert!(
        matches!(client.recv(5 * interval), Received::Silence),
        "no event while idle"
    );
}

#[test]
fn ac5_cancel_answers_cancelled_within_two_seconds_and_calls_the_cancel_path() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_secs(30)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();
    client.send(render_still());
    wait_until("the job to start", SECOND, || {
        fixture.handler.running.load(Ordering::SeqCst) == 1
    });

    let sent = Instant::now();
    client.send(Request::Cancel);
    let reply = client.expect_response(2 * SECOND);

    assert_eq!(reply, Response::Cancelled);
    assert!(sent.elapsed() < 2 * SECOND);
    assert_eq!(fixture.handler.cancels(), 1);
    client.send(hello());
    assert!(matches!(
        client.expect_response(SECOND),
        Response::Capabilities(_)
    ));
}

#[test]
fn ac5_connection_loss_cancels_without_a_reply_and_the_next_connection_is_served() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_secs(30)),
        Duration::from_secs(30),
    );
    let mut first = fixture.connect();
    first.send(render_still());
    wait_until("the job to start", SECOND, || {
        fixture.handler.running.load(Ordering::SeqCst) == 1
    });

    first
        .stream
        .shutdown(std::net::Shutdown::Write)
        .expect("close the write half");

    wait_until("the cancel path", 2 * SECOND, || {
        fixture.handler.cancels() == 1
    });
    assert!(
        matches!(first.recv(SECOND), Received::Closed),
        "connection loss gets no reply"
    );
    let mut second = fixture.connect();
    second.send(hello());
    assert!(matches!(
        second.expect_response(SECOND),
        Response::Capabilities(_)
    ));
    second.send(render_still());
    wait_until("the second job", SECOND, || {
        fixture.handler.jobs.lock().expect("jobs").len() == 2
    });
    assert_eq!(fixture.handler.running.load(Ordering::SeqCst), 1);
}

#[test]
fn cancel_on_an_idle_session_is_a_no_op_without_a_reply() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(10)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();

    client.send(Request::Cancel);
    client.send(hello());

    assert!(matches!(
        client.expect_response(SECOND),
        Response::Capabilities(_)
    ));
    assert!(matches!(
        client.recv(Duration::from_millis(200)),
        Received::Silence
    ));
}

#[test]
fn ac7_photo_request_runs_with_the_photo_ceilings() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(10)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();

    client.send(render_still());
    client.expect_response(SECOND);

    let job = fixture.handler.last_job();
    assert_eq!(job.class, LimitClass::Photo);
    assert_eq!(job.limits, ceilings().photo);
    assert_eq!(job.limits.wall, Duration::from_secs(3));
    assert_eq!(job.limits.memory_bytes, 128 * MIB);
    assert_eq!(job.limits.cpu, 2);
}

#[test]
fn ac7_video_request_runs_with_the_video_ceilings() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(10)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();

    client.send(Request::Transcode {
        job_dir: "/scratch/job".into(),
        plan: Plan::Remux,
        limits: greedy_limits(),
    });
    client.expect_response(SECOND);

    let job = fixture.handler.last_job();
    assert_eq!(job.class, LimitClass::Video);
    assert_eq!(job.limits, ceilings().video);
    assert!(matches!(job.spec, JobSpec::Transcode { .. }));
}

#[test]
fn ac7_limits_below_the_ceiling_are_kept() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(10)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();
    let mut limits = greedy_limits();
    limits.wall = 1500;
    limits.cpu = 1;

    client.send(Request::RenderStill {
        job_dir: "/scratch/job".into(),
        max_edge: 320,
        poster_at_ms: None,
        limits,
    });
    client.expect_response(SECOND);

    let job = fixture.handler.last_job();
    assert_eq!(job.limits.wall, Duration::from_millis(1500));
    assert_eq!(job.limits.cpu, 1);
    assert_eq!(
        job.limits.max_output_bytes,
        ceilings().photo.max_output_bytes
    );
}

#[test]
fn a_probe_runs_under_the_photo_ceilings() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(10)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();

    client.send(Request::Probe {
        job_dir: "/scratch/job".into(),
    });
    client.expect_response(SECOND);

    assert_eq!(fixture.handler.last_job().limits, ceilings().photo);
}

#[test]
fn a_violation_is_answered_and_ends_the_runner() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(10)),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();

    client.send_payload(br#"{"request":{"hello":{"protocol":1,"extra":1}}}"#);
    let reply = client.expect_response(SECOND);

    assert_eq!(reply, Response::Error(DecodeError::ProtocolViolation));
    assert_eq!(fixture.outcome().expect("serve"), Exit::ProtocolViolation);
}

#[test]
fn a_job_that_ignores_cancel_ends_the_runner_instead_of_overlapping_the_next_job() {
    let handler = Scripted::new(Duration::from_secs(10)).ignoring_cancel();
    let fixture = Fixture::start_with(handler, Duration::from_secs(30), |config| {
        config.cancel_wait = Duration::from_millis(200);
    });
    let mut client = fixture.connect();
    client.send(render_still());
    wait_until("the job to start", SECOND, || {
        fixture.handler.running.load(Ordering::SeqCst) == 1
    });

    drop(client);

    let outcome = fixture.outcome();
    assert!(matches!(outcome, Err(RunnerError::JobStuck)), "{outcome:?}");
}

#[test]
fn a_panicking_job_ends_the_runner() {
    let fixture = Fixture::start(
        Scripted::new(Duration::from_millis(10)).panicking(),
        Duration::from_secs(30),
    );
    let mut client = fixture.connect();

    client.send(render_still());

    let outcome = fixture.outcome();
    assert!(
        matches!(outcome, Err(RunnerError::JobPanicked)),
        "{outcome:?}"
    );
}
