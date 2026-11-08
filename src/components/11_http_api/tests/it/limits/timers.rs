use std::time::{Duration, Instant};

use http_body_util::BodyExt;
use hyper::Request;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use super::support::*;

const CONTENT_LENGTH_10: &str = "content-length: 10\r\n";

fn never_a_success(answer: &[u8]) {
    assert!(
        !matches!(status_of(answer), Some(200..=299)),
        "the request must not end in a clean answer"
    );
}

#[tokio::test]
async fn a_block_body_that_goes_quiet_ends_at_the_idle_timeout_and_the_port_is_not_called() {
    // Arrange
    let server = Limited::start().await;
    let mut raw = Raw::connect(server.public.addr).await;
    raw.send(&Raw::head(
        "POST",
        path_of(sunnycloud_http_api::registry::BodyClass::Block),
        CONTENT_LENGTH_10,
        "",
    ))
    .await;
    let started = Instant::now();

    // Act
    raw.send(b"x").await;
    let answer = raw.finish(ANSWER_BOUND).await;
    let elapsed = started.elapsed();

    // Assert
    never_a_success(&answer);
    assert!(elapsed >= TIMER, "ended after {elapsed:?}");
    assert!(elapsed < TIMER * 8, "ended after {elapsed:?}");
    assert_eq!(server.port_calls(), 0);
    server.stop().await;
}

#[tokio::test]
async fn a_slow_handler_ends_at_the_request_timeout() {
    // Arrange
    let server = Limited::start().await;
    let mut raw = Raw::connect(server.public.addr).await;
    raw.send(&Raw::head("POST", SLOW_PATH, "content-length: 0\r\n", ""))
        .await;
    let started = Instant::now();

    // Act
    let answer = raw.finish(ANSWER_BOUND).await;
    let elapsed = started.elapsed();

    // Assert
    never_a_success(&answer);
    assert!(elapsed >= TIMER, "ended after {elapsed:?}");
    assert!(elapsed < TIMER * 8, "ended after {elapsed:?}");
    server.stop().await;
}

#[tokio::test]
async fn a_timed_out_request_is_reset_not_ended_with_a_clean_empty_body() {
    // Arrange
    let server = Limited::start().await;
    let stream = TcpStream::connect(server.public.addr)
        .await
        .expect("tcp connect");
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .expect("h1 handshake");
    tokio::spawn(connection);
    let request = Request::post(SLOW_PATH)
        .header("host", "sunnycloud.test")
        .body(http_body_util::Empty::<hyper::body::Bytes>::new())
        .expect("request");

    // Act
    let outcome = match sender.send_request(request).await {
        Ok(response) => response.into_body().collect().await.map(|_| ()),
        Err(error) => Err(error),
    };

    // Assert
    assert!(outcome.is_err(), "the answer must not complete");
    server.stop().await;
}

#[tokio::test]
async fn a_block_put_has_no_total_cap_only_the_idle_timer() {
    // Arrange
    let server = Limited::start().await;
    let mut raw = Raw::connect(server.public.addr).await;

    // Act
    raw.send(&Raw::head(
        "POST",
        SLOW_BLOCK_PATH,
        "content-length: 2\r\n",
        "",
    ))
    .await;
    raw.send(b"ok").await;
    let answer = raw.finish(ANSWER_BOUND).await;

    // Assert
    assert_eq!(
        status_of(&answer),
        Some(200),
        "the handler took twice the request timeout"
    );
    server.stop().await;
}

#[tokio::test]
async fn long_polls_and_downloads_keep_their_own_clocks() {
    // Arrange
    let server = Limited::start().await;
    let wait = |path: &str| format!("{path}?wait=1");
    let get = |path: String| {
        let addr = server.public.addr;
        async move {
            let started = Instant::now();
            let mut raw = Raw::connect(addr).await;
            raw.send(&Raw::head("GET", &path, "", "")).await;
            let answer = raw.finish(ANSWER_BOUND).await;
            (answer, started.elapsed())
        }
    };

    // Act
    let (answered, silent, download) = tokio::join!(
        get(wait(POLL_ANSWERED_PATH)),
        get(wait(POLL_SILENT_PATH)),
        get(DOWNLOAD_PATH.to_owned()),
    );

    // Assert
    assert_eq!(status_of(&answered.0), Some(200));
    assert!(answered.1 > TIMER, "answered after {:?}", answered.1);
    never_a_success(&silent.0);
    assert!(
        silent.1 >= Duration::from_secs(6) - Duration::from_millis(50),
        "the silent long-poll ended after {:?}, before wait + 5 s",
        silent.1
    );
    assert_eq!(status_of(&download.0), Some(200));
    let text = String::from_utf8_lossy(&download.0).into_owned();
    assert!(text.contains("first-") && text.contains("second-") && text.contains("third"));
    assert!(
        download.1 > TIMER * 3,
        "the stream outlasted request_timeout"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_download_to_a_peer_that_stops_reading_is_cut_at_the_write_idle_limit() {
    // Arrange
    let server = Limited::start().await;
    let mut raw = Raw::connect(server.public.addr).await;
    raw.send(&Raw::head("GET", BIG_DOWNLOAD_PATH, "", "")).await;

    // Act
    tokio::time::sleep(TIMER * 3).await;
    let received = raw.finish(ANSWER_BOUND).await;

    // Assert
    assert!(
        received.len() < BIG_DOWNLOAD_BYTES,
        "the server kept writing to a peer that did not read: {} bytes",
        received.len()
    );
    server.stop().await;
}
