//! AZ-3871 AC-4 (typed errors only) and the job calls against the real runner socket loop: events,
//! cancel, and the rule that an abandoned call closes the connection.

mod decoder_support;

use std::future::pending;
use std::sync::atomic::Ordering;
use std::time::Duration;

use decoder_support::{Handler, Runner, eventually, within};
use media_shared::decoder_protocol::{
    DecodeError, Event, ImageResult, LimitKind, Limits, Response, TranscodeResult,
};
use media_shared::policy::{AudioPlan, Encoder, MediaKind, Plan, Probe};
use sunnycloud_worker::{CallError, DecoderConn};
use tempfile::TempDir;

const ALIVE: Duration = Duration::from_millis(20);
const WAIT: Duration = Duration::from_secs(10);

fn limits() -> Limits {
    Limits {
        cpu: 1,
        memory_bytes: 1 << 20,
        wall: 10_000,
        stall_timeout: 10_000,
        max_output_bytes: 1 << 20,
    }
}

fn image() -> ImageResult {
    ImageResult {
        width: 320,
        height: 240,
        bytes: 1234,
    }
}

async fn start(answer: Response) -> (TempDir, std::sync::Arc<Handler>, Runner, DecoderConn) {
    let dir = tempfile::tempdir().expect("temporary folder");
    let socket = dir.path().join("decoder.sock");
    let handler = Handler::new(answer);
    let runner = Runner::start(&socket, &handler, ALIVE);
    let conn = DecoderConn::connect(&socket).await.expect("connect");
    (dir, handler, runner, conn)
}

#[tokio::test]
async fn every_typed_decoder_failure_reaches_the_caller_as_its_own_variant() {
    let cases = [
        DecodeError::Unsupported,
        DecodeError::Corrupt,
        DecodeError::LimitExceeded {
            limit: LimitKind::Wall,
        },
        DecodeError::Stalled,
        DecodeError::EncoderFailed { exit_code: 1 },
        DecodeError::OutputInvalid,
        DecodeError::Busy,
    ];
    for expected in cases {
        // Arrange
        let (_dir, _handler, _runner, mut conn) = start(Response::Error(expected)).await;

        // Act
        let result = conn
            .render_still("/scratch/job", 320, None, limits(), &mut |_| {}, pending())
            .await;

        // Assert
        match result {
            Err(CallError::Decode(got)) => assert_eq!(got, expected),
            other => panic!("{expected:?}: got {other:?}"),
        }
        assert!(
            conn.hello().await.is_ok(),
            "{expected:?}: a typed failure must leave the connection usable"
        );
    }
}

#[tokio::test]
async fn a_finished_job_returns_its_typed_result() {
    // Arrange
    let (_dir, handler, _runner, mut conn) = start(Response::ImageResult(image())).await;

    // Act
    let result = conn
        .render_still(
            "/scratch/job",
            320,
            Some(1500),
            limits(),
            &mut |_| {},
            pending(),
        )
        .await;

    // Assert
    assert_eq!(result.expect("the still"), image());
    assert_eq!(handler.log(), ["job"]);
}

#[tokio::test]
async fn alive_and_progress_events_reach_the_caller_while_the_job_runs() {
    // Arrange
    let (_dir, handler, _runner, mut conn) = start(Response::ImageResult(image())).await;
    *handler.run_for.lock().expect("run_for") = Duration::from_millis(150);
    handler.progress_lines.store(2, Ordering::SeqCst);
    let mut events = Vec::new();

    // Act
    let result = conn
        .render_still(
            "/scratch/job",
            320,
            None,
            limits(),
            &mut |event| events.push(event),
            pending(),
        )
        .await;

    // Assert
    assert!(result.is_ok());
    assert!(events.contains(&Event::Alive), "no Alive in {events:?}");
    assert!(events.contains(&Event::Progress {
        out_time_ms: 1000,
        speed: 1.5
    }));
    assert!(events.contains(&Event::Progress {
        out_time_ms: 2000,
        speed: 1.5
    }));
}

#[tokio::test]
async fn the_cancel_future_cancels_the_job_and_leaves_the_connection_usable() {
    // Arrange
    let (_dir, handler, _runner, mut conn) = start(Response::ImageResult(image())).await;
    *handler.run_for.lock().expect("run_for") = Duration::from_secs(30);

    // Act
    let result = within(
        WAIT,
        conn.render_still(
            "/scratch/job",
            320,
            None,
            limits(),
            &mut |_| {},
            tokio::time::sleep(Duration::from_millis(60)),
        ),
    )
    .await
    .expect("the cancelled job ends");

    // Assert
    assert!(matches!(result, Err(CallError::Cancelled)), "{result:?}");
    assert_eq!(handler.cancels.load(Ordering::SeqCst), 1);
    assert!(conn.hello().await.is_ok());
}

#[tokio::test]
async fn dropping_a_running_call_closes_the_connection_and_cancels_the_job() {
    // Arrange
    let (_dir, handler, _runner, mut conn) = start(Response::ImageResult(image())).await;
    *handler.run_for.lock().expect("run_for") = Duration::from_secs(30);

    // Act
    let abandoned = tokio::time::timeout(
        Duration::from_millis(80),
        conn.render_still("/scratch/job", 320, None, limits(), &mut |_| {}, pending()),
    )
    .await;

    // Assert
    assert!(abandoned.is_err());
    assert!(!conn.is_open());
    assert!(matches!(conn.hello().await, Err(CallError::Disconnected)));
    assert!(
        eventually(WAIT, || handler.cancels.load(Ordering::SeqCst) == 1).await,
        "the runner never saw the job cancelled by the closed socket"
    );
}

#[tokio::test]
async fn a_probe_answered_with_the_wrong_result_kind_is_a_violation() {
    // Arrange
    let (_dir, _handler, _runner, mut conn) = start(Response::ImageResult(image())).await;

    // Act
    let result = conn.probe("/scratch/job", &mut |_| {}, pending()).await;

    // Assert
    assert!(matches!(result, Err(CallError::Violation(_))), "{result:?}");
    assert!(!conn.is_open());
}

fn video_probe() -> Probe {
    Probe {
        kind: MediaKind::Video,
        container: "mp4".into(),
        moov_first: true,
        video: None,
        audio: None,
        duration_ms: Some(1000),
    }
}

#[tokio::test]
async fn probe_and_transcode_return_their_typed_results() {
    // Arrange
    let (_dir, handler, _runner, mut conn) = start(Response::Probe(video_probe())).await;
    let plan = Plan::Transcode {
        encoder: Encoder::Cpu,
        scale_to_short_edge: 720,
        tone_map: false,
        audio: AudioPlan::Aac,
        faststart: true,
    };

    // Act
    let probed = conn.probe("/scratch/job", &mut |_| {}, pending()).await;
    *handler.answer.lock().expect("answer") = Response::TranscodeResult(TranscodeResult {
        probe_of_output: video_probe(),
        bytes: 99,
    });
    let transcoded = conn
        .transcode("/scratch/job", plan, limits(), &mut |_| {}, pending())
        .await;

    // Assert
    assert_eq!(probed.expect("probe"), video_probe());
    let transcoded = transcoded.expect("transcode");
    assert_eq!(transcoded.bytes, 99);
    assert_eq!(transcoded.probe_of_output, video_probe());
}
