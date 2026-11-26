//! AC-1: a parsed script replays over a real Unix socket exactly, in order, and the default answers every request
//! after the queue. Each hung or connection-level answer gets a connection of its own.

mod support;

use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{
    Capabilities, DecodeError, Event, ImageResult, Reply, Request, Response, SelfCheckItem,
    SelfCheckReport, TranscodeResult,
};
use media_shared::policy::{MediaKind, Probe};
use serde_json::json;
use sunnycloud_fake_decoder::{Config, FakeDecoder, Script, capabilities};
use support::{Client, JobDir, WAIT, hello, probe, render, self_check, wait_until};
use tempfile::TempDir;

fn photo(container: &str) -> Probe {
    Probe {
        kind: MediaKind::Photo,
        container: container.into(),
        moov_first: false,
        video: None,
        audio: None,
        duration_ms: None,
    }
}

fn photo_json(container: &str) -> serde_json::Value {
    json!({"kind": "photo", "container": container, "moov_first": false, "video": null, "audio": null,
           "duration_ms": null})
}

struct Fd {
    decoder: FakeDecoder,
    socket: PathBuf,
    _dir: TempDir,
}

fn start(script: serde_json::Value) -> Fd {
    let dir = tempfile::tempdir().expect("socket folder");
    let socket = dir.path().join("decoder.sock");
    let script = Script::parse(&script.to_string()).expect("the script parses");
    let decoder = FakeDecoder::serve(Config {
        script,
        ..Config::new(&socket)
    })
    .expect("FD binds");
    Fd {
        decoder,
        socket,
        _dir: dir,
    }
}

fn response(reply: Option<Reply>) -> Response {
    match reply {
        Some(Reply::Response(response)) => response,
        other => panic!("expected a response, got {other:?}"),
    }
}

async fn ask(client: &mut Client, request: Request) -> Response {
    client.send(request).await;
    response(client.reply().await)
}

#[tokio::test]
async fn scripted_answers_replay_in_order_then_the_default() {
    // Arrange
    let fd = start(json!({
        "hello": {"build_timestamp": 1_690_000_000u64},
        "self_check": [{"name": "no_network", "ok": false}],
        "probe": {"queue": [{"probe": photo_json("jpeg")}, {"error": "unsupported"}],
                  "default": {"probe": photo_json("png")}},
        "render_still": {"queue": [{"image": {"width": 1440, "height": 1080}},
                                   {"over_time": {"delay_ms": 50, "answer": {"image": {"width": 320, "height": 240}}}}],
                         "default": {"error": "corrupt"}},
        "transcode": {"default": {"transcode": {"probe_of_output": photo_json("mp4")}}},
    }));
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;

    // Act
    let mut answers = vec![
        ask(&mut client, hello()).await,
        ask(&mut client, self_check()).await,
    ];
    for _ in 0..4 {
        answers.push(ask(&mut client, probe(&job)).await);
    }
    answers.push(ask(&mut client, render(&job, 1440)).await);
    let first_render = fs::read(job.out("result.webp")).expect("out/result.webp");
    let started = Instant::now();
    answers.push(ask(&mut client, render(&job, 320)).await);
    let over_time = started.elapsed();
    answers.push(ask(&mut client, render(&job, 320)).await);
    let transcode = Request::Transcode {
        job_dir: job.dir(),
        plan: media_shared::policy::Plan::Remux,
        limits: support::limits(),
    };
    answers.push(ask(&mut client, transcode).await);

    // Assert
    let expected = [
        Response::Capabilities(Capabilities {
            build_timestamp: 1_690_000_000,
            ..capabilities()
        }),
        Response::SelfCheck(SelfCheckReport(vec![SelfCheckItem {
            name: "no_network".into(),
            ok: false,
        }])),
        Response::Probe(photo("jpeg")),
        Response::Error(DecodeError::Unsupported),
        Response::Probe(photo("png")),
        Response::Probe(photo("png")),
        Response::ImageResult(ImageResult {
            width: 1440,
            height: 1080,
            bytes: 14,
        }),
        Response::ImageResult(ImageResult {
            width: 320,
            height: 240,
            bytes: 13,
        }),
        Response::Error(DecodeError::Corrupt),
        Response::TranscodeResult(TranscodeResult {
            probe_of_output: photo("mp4"),
            bytes: 10,
        }),
    ];
    assert_eq!(answers, expected);
    assert_eq!(first_render, b"webp edge 1440");
    assert_eq!(
        fs::read(job.out("result.mp4")).expect("out/result.mp4"),
        b"mp4 output"
    );
    assert!(
        over_time >= Duration::from_millis(50),
        "answered after {over_time:?}"
    );
    assert_eq!(fd.decoder.requests().len(), 8);
}

#[tokio::test]
async fn hang_reads_the_request_and_closes_the_connection() {
    // Arrange
    let fd = start(json!({"probe": {"default": "hang"}}));
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;

    // Act
    client.send(probe(&job)).await;
    let reply = client.reply().await;

    // Assert
    assert_eq!(reply, None);
    assert_eq!(fd.decoder.requests(), [probe(&job)]);
}

#[tokio::test]
async fn silent_sends_nothing_and_ignores_cancel() {
    // Arrange
    let fd = start(json!({"probe": {"default": "silent"}}));
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;

    // Act
    client.send(probe(&job)).await;
    client.send(Request::Cancel).await;
    wait_until("FD to take the cancel", || fd.decoder.cancels().len() == 1).await;

    // Assert
    assert!(client.nothing_pending(), "FD sent a frame");
}

#[tokio::test]
async fn alive_only_sends_alive_and_never_answers_even_after_cancel() {
    // Arrange
    let fd = start(json!({"probe": {"default": {"alive_only": {"every_ms": 10}}}}));
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;
    client.send(probe(&job)).await;

    // Act
    let before = [client.reply().await, client.reply().await];
    client.send(Request::Cancel).await;
    wait_until("FD to take the cancel", || fd.decoder.cancels().len() == 1).await;
    let after = [client.reply().await, client.reply().await];

    // Assert
    let alive = Some(Reply::Event(Event::Alive));
    assert_eq!(before, [alive.clone(), alive.clone()]);
    assert_eq!(after, [alive.clone(), alive]);
}

#[tokio::test]
async fn garbage_writes_exactly_the_scripted_bytes() {
    // Arrange
    let fd = start(
        json!({"probe": {"default": {"garbage": {"framed": true, "chunks": [{"hex": "7bfffe7d"}]}}}}),
    );
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;

    // Act
    client.send(probe(&job)).await;
    let bytes = client.raw(8).await;

    // Assert
    assert_eq!(bytes, [0, 0, 0, 4, 0x7b, 0xff, 0xfe, 0x7d]);
}

#[tokio::test]
async fn over_time_sends_alive_meanwhile_and_answers_cancel_with_cancelled() {
    // Arrange
    let fd = start(json!({"probe": {"default":
        {"over_time": {"delay_ms": 60_000, "alive_every_ms": 10, "answer": {"error": "corrupt"}}}}}));
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;
    client.send(probe(&job)).await;

    // Act
    let alive = client.reply().await;
    client.send(Request::Cancel).await;
    let mut end = client.reply().await;
    while end == Some(Reply::Event(Event::Alive)) {
        end = client.reply().await;
    }

    // Assert
    assert_eq!(alive, Some(Reply::Event(Event::Alive)));
    assert_eq!(end, Some(Reply::Response(Response::Cancelled)));
}

async fn connect_refused(socket: &Path) -> bool {
    tokio::net::UnixStream::connect(socket).await.is_err()
}

/// Polls until a connect succeeds; returns how long that took.
async fn until_listening(socket: &Path) -> Duration {
    let started = Instant::now();
    while connect_refused(socket).await {
        assert!(started.elapsed() < WAIT, "FD never listened again");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    started.elapsed()
}

#[tokio::test]
async fn outage_closes_the_connection_refuses_for_the_time_then_listens_again() {
    // Arrange
    let fd = start(
        json!({"probe": {"queue": [{"outage": {"refuse_ms": 300}}], "default": {"error": "corrupt"}}}),
    );
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;
    let mut bystander = Client::connect(&fd.socket).await;
    let started = Instant::now();

    // Act
    client.send(probe(&job)).await;
    let closed = client.reply().await;
    let bystander_closed = bystander.reply().await;
    wait_until("the listener to close", || {
        std::os::unix::net::UnixStream::connect(&fd.socket).is_err()
    })
    .await;
    until_listening(&fd.socket).await;
    let down_for = started.elapsed();
    let mut again = Client::connect(&fd.socket).await;
    let next = ask(&mut again, probe(&job)).await;

    // Assert
    assert_eq!((closed, bystander_closed), (None, None));
    assert!(
        down_for >= Duration::from_millis(300),
        "listening again after {down_for:?}"
    );
    assert_eq!(next, Response::Error(DecodeError::Corrupt));
    let mode = fs::metadata(&fd.socket)
        .expect("socket")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o660);
}

#[tokio::test]
async fn idle_outage_starts_after_the_first_self_check_and_only_once() {
    // Arrange
    let fd = start(json!({"idle_outage": {"after_ms": 20, "refuse_ms": 200}}));
    let mut client = Client::connect(&fd.socket).await;
    ask(&mut client, hello()).await;
    ask(&mut client, self_check()).await;
    let answered = Instant::now();

    // Act
    let closed = client.reply().await;
    until_listening(&fd.socket).await;
    let down_for = answered.elapsed();
    let mut again = Client::connect(&fd.socket).await;
    let handshake = [
        ask(&mut again, hello()).await,
        ask(&mut again, self_check()).await,
    ];
    let still_open = ask(&mut again, Request::Cancel).await;

    // Assert
    assert_eq!(closed, None);
    assert!(
        down_for >= Duration::from_millis(200),
        "listening again after {down_for:?}"
    );
    assert!(matches!(handshake[1], Response::SelfCheck(_)));
    assert_eq!(still_open, Response::Cancelled);
    // The connect that found FD listening again is a connection of its own, so the second client's number is
    // only known to be later than the first's.
    let conns: Vec<u64> = fd
        .decoder
        .records()
        .iter()
        .map(|record| record.conn)
        .collect();
    assert_eq!(conns.len(), 5, "{conns:?}");
    assert_eq!(conns[..2], [1, 1]);
    assert!(
        conns[2] > 1 && conns[2..].iter().all(|conn| *conn == conns[2]),
        "{conns:?}"
    );
}

#[tokio::test]
async fn a_hostile_output_is_planted_before_the_answer() {
    // Arrange
    let fd = start(
        json!({"render_still": {"default": {"image": {"width": 320, "height": 240, "output": "fifo"}}}}),
    );
    let job = JobDir::new("job-1");
    let mut client = Client::connect(&fd.socket).await;

    // Act
    let answer = ask(&mut client, render(&job, 320)).await;

    // Assert
    assert_eq!(
        answer,
        Response::ImageResult(ImageResult {
            width: 320,
            height: 240,
            bytes: 1,
        })
    );
    let planted = fs::symlink_metadata(job.out("result.webp")).expect("planted");
    assert!(planted.file_type().is_fifo());
}

#[tokio::test]
async fn an_output_fd_cannot_plant_closes_the_connection_without_an_answer() {
    // Arrange
    let fd = start(json!({"render_still": {"default": {"image": {"width": 320, "height": 240}}}}));
    let job = JobDir::new("job-1");
    fs::remove_dir(job.path.join("out")).expect("out/ removed");
    let mut client = Client::connect(&fd.socket).await;

    // Act
    client.send(render(&job, 320)).await;
    let reply = client.reply().await;

    // Assert
    assert_eq!(reply, None);
}
