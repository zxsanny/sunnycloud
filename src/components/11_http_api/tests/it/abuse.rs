use std::time::{Duration, Instant};

use sunnycloud_http_api::HttpConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::harness::raw_h2::{
    END_HEADERS, END_STREAM, Frame, RawH2, Read, encode_frame, frame_type, is_success_status,
    request_block,
};
use crate::harness::{Server, default_config};

/// How long the server may stay silent before the test decides it has said all it will.
const WIRE_WAIT: Duration = Duration::from_secs(2);
const ENHANCE_YOUR_CALM: u32 = 0xb;

fn abuse_config() -> HttpConfig {
    HttpConfig {
        h1_header_timeout: Duration::from_secs(1),
        h2_max_pending_reset: 20,
        ..default_config()
    }
}

/// What the server did with the abusive h2 peer.
#[derive(Debug, Default)]
struct Outcome {
    goaway_codes: Vec<u32>,
    stream_resets: usize,
    success_responses: usize,
    other_responses: usize,
    closed: bool,
}

/// Reads until the server closes the connection or goes quiet.
async fn collect(raw: &mut RawH2) -> Outcome {
    let mut outcome = Outcome::default();
    loop {
        match raw.read(WIRE_WAIT).await {
            Read::Frame(Frame { kind, payload, .. }) if kind == frame_type::GOAWAY => {
                outcome.goaway_codes.push(u32::from_be_bytes([
                    payload[4], payload[5], payload[6], payload[7],
                ]));
            }
            Read::Frame(Frame { kind, .. }) if kind == frame_type::RST_STREAM => {
                outcome.stream_resets += 1;
            }
            Read::Frame(Frame { kind, payload, .. }) if kind == frame_type::HEADERS => {
                if is_success_status(&payload) {
                    outcome.success_responses += 1;
                } else {
                    outcome.other_responses += 1;
                }
            }
            Read::Frame(_) => {}
            Read::Closed => {
                outcome.closed = true;
                return outcome;
            }
            Read::Quiet => return outcome,
        }
    }
}

#[tokio::test]
async fn an_h1_client_dripping_headers_is_closed_within_two_seconds() {
    // Arrange
    let server = Server::start(abuse_config()).await;
    let stream = TcpStream::connect(server.public)
        .await
        .expect("tcp connect");
    let (mut reader, mut writer) = stream.into_split();
    let started = Instant::now();
    writer
        .write_all(b"GET / HTTP/1.1\r\nHost: sunnycloud.test\r\nX-Slow: ")
        .await
        .expect("first bytes");

    // Act
    let mut drip = tokio::time::interval(Duration::from_millis(300));
    let mut buffer = [0u8; 512];
    let closed_after = loop {
        tokio::select! {
            read = reader.read(&mut buffer) => {
                match read {
                    Ok(0) | Err(_) => break started.elapsed(),
                    Ok(_) => {}
                }
            }
            _ = drip.tick() => {
                if writer.write_all(b"a").await.is_err() {
                    break started.elapsed();
                }
            }
        }
    };
    server.stop().await.expect("run returns");

    // Assert
    assert!(
        closed_after < Duration::from_secs(2),
        "closed after {closed_after:?}"
    );
}

#[tokio::test]
async fn a_thousand_headers_and_reset_pairs_end_with_goaway_and_close() {
    // Arrange
    let server = Server::start(abuse_config()).await;
    let mut raw = RawH2::connect(server.public).await;
    let block = request_block(&[]);
    let mut burst = Vec::new();
    for pair in 0..1000u32 {
        let stream = 2 * pair + 1;
        burst.extend(encode_frame(
            frame_type::HEADERS,
            END_HEADERS | END_STREAM,
            stream,
            &block,
        ));
        burst.extend(encode_frame(
            frame_type::RST_STREAM,
            0,
            stream,
            &8u32.to_be_bytes(),
        ));
    }

    // Act
    let write = raw.write(&burst).await;
    let outcome = collect(&mut raw).await;
    server.stop().await.expect("run returns");

    // Assert
    assert!(
        write.is_ok() || outcome.closed,
        "the burst was neither accepted nor answered with a close"
    );
    assert!(
        outcome.goaway_codes.contains(&ENHANCE_YOUR_CALM),
        "{outcome:?}"
    );
    assert!(outcome.closed, "{outcome:?}");
}

#[tokio::test]
async fn a_continuation_flood_is_refused_and_never_answered_with_a_success() {
    // Arrange
    let server = Server::start(abuse_config()).await;
    let mut raw = RawH2::connect(server.public).await;
    let junk = vec![b'a'; 16_000];

    // Act
    let mut write_failed = false;
    if raw
        .send_frame(frame_type::HEADERS, 0, 1, &request_block(&[]))
        .await
        .is_err()
    {
        write_failed = true;
    }
    for _ in 0..200 {
        if write_failed {
            break;
        }
        write_failed = raw
            .send_frame(frame_type::CONTINUATION, 0, 1, &junk)
            .await
            .is_err();
    }
    let outcome = collect(&mut raw).await;
    server.stop().await.expect("run returns");

    // Assert
    assert_eq!(outcome.success_responses, 0, "{outcome:?}");
    assert!(
        outcome.closed
            || write_failed
            || !outcome.goaway_codes.is_empty()
            || outcome.stream_resets > 0
            || outcome.other_responses > 0,
        "the flood was not refused: {outcome:?}"
    );
}

#[tokio::test]
async fn a_seventeen_kib_header_list_is_refused_and_never_answered_with_a_success() {
    // Arrange
    let server = Server::start(abuse_config()).await;
    let mut raw = RawH2::connect(server.public).await;
    let value = vec![b'a'; 3_500];
    let block = request_block(&[
        ("x-a", &value),
        ("x-b", &value),
        ("x-c", &value),
        ("x-d", &value),
        ("x-e", &value),
    ]);
    let (first, rest) = block.split_at(8_000);

    // Act
    let sent = async {
        raw.send_frame(frame_type::HEADERS, END_STREAM, 1, first)
            .await?;
        raw.send_frame(frame_type::CONTINUATION, END_HEADERS, 1, rest)
            .await
    }
    .await;
    let outcome = collect(&mut raw).await;
    server.stop().await.expect("run returns");

    // Assert
    assert_eq!(outcome.success_responses, 0, "{outcome:?}");
    assert!(
        sent.is_err()
            || outcome.stream_resets > 0
            || !outcome.goaway_codes.is_empty()
            || outcome.other_responses > 0,
        "the oversized header list was not refused: {outcome:?}"
    );
}
