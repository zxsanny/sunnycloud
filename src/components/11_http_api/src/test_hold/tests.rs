use std::collections::VecDeque;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Bytes;
use contracts::{ByteStream, StreamError};
use futures_core::Stream;
use serde_json::json;
use uuid::Uuid;

use super::{HoldRequest, HoldRoute, HoldStatus, WorkerHold, hold_request};

const BOUND: Duration = Duration::from_secs(5);

/// An in-memory body of `sizes` chunks; byte `i` of the whole body is `i % 251`.
struct Chunks(VecDeque<Bytes>);

impl Stream for Chunks {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.get_mut().0.pop_front().map(Ok))
    }
}

fn body(sizes: &[usize]) -> (ByteStream, Vec<u8>) {
    let total: usize = sizes.iter().sum();
    let whole: Vec<u8> = (0..total)
        .map(|i| u8::try_from(i % 251).expect("a byte"))
        .collect();
    let mut at = 0;
    let mut chunks = VecDeque::new();
    for size in sizes {
        chunks.push_back(Bytes::copy_from_slice(&whole[at..at + size]));
        at += size;
    }
    (Box::pin(Chunks(chunks)), whole)
}

async fn next(stream: &mut ByteStream) -> Option<Bytes> {
    poll_fn(|cx| stream.as_mut().poll_next(cx))
        .await
        .map(|chunk| chunk.expect("a chunk"))
}

async fn rest(mut stream: ByteStream) -> Vec<u8> {
    let mut read = Vec::new();
    while let Some(chunk) = next(&mut stream).await {
        read.extend_from_slice(&chunk);
    }
    read
}

fn armed(route: HoldRoute, kind: Option<&str>, after_bytes: u64) -> Arc<WorkerHold> {
    let hold = Arc::new(WorkerHold::default());
    assert!(hold.arm(HoldRequest {
        route,
        kind: kind.map(str::to_owned),
        after_bytes,
    }));
    hold
}

async fn wait_held(hold: &WorkerHold) -> HoldStatus {
    let deadline = tokio::time::Instant::now() + BOUND;
    loop {
        let status = hold.status();
        if status.held {
            return status;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never held: {status:?}"
        );
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn the_stream_passes_its_bytes_splitting_a_chunk_then_waits_for_the_release() {
    // Arrange
    let hold = armed(HoldRoute::Input, None, 50);
    let job = Uuid::from_u128(1);
    let (source, whole) = body(&[40, 40, 40]);
    let mut stream = hold.wrap(HoldRoute::Input, None, job, source);

    // Act
    let first = next(&mut stream).await.expect("the first chunk");
    let second = next(&mut stream).await.expect("the cut chunk");
    let reader = tokio::spawn(rest(stream));
    let status = wait_held(&hold).await;
    let unfinished = !reader.is_finished();
    hold.release();
    let after = tokio::time::timeout(BOUND, reader)
        .await
        .expect("the rest after the release")
        .expect("the reader");

    // Assert
    assert_eq!((first.len(), second.len()), (40, 10));
    assert_eq!(
        status,
        HoldStatus {
            armed: false,
            held: true,
            job_id: Some(job),
            bytes: 50,
        }
    );
    assert!(unfinished, "the stream went on while held");
    let mut read = [first.to_vec(), second.to_vec()].concat();
    read.extend(after);
    assert_eq!(read, whole);
    assert_eq!(hold.status(), HoldStatus::default());
}

#[tokio::test]
async fn the_hold_is_one_shot_and_takes_only_its_route_and_kind() {
    // Arrange
    let hold = armed(HoldRoute::Output, Some("thumb"), 0);
    let (job, other) = (Uuid::from_u128(2), Uuid::from_u128(3));

    // Act
    let input = rest(hold.wrap(HoldRoute::Input, None, job, body(&[10]).0)).await;
    let preview = rest(hold.wrap(HoldRoute::Output, Some("preview"), job, body(&[10]).0)).await;
    let still_armed = hold.status();
    let taken = hold.wrap(HoldRoute::Output, Some("thumb"), job, body(&[10]).0);
    let second = rest(hold.wrap(HoldRoute::Output, Some("thumb"), other, body(&[10]).0)).await;
    let rearm = hold.arm(HoldRequest {
        route: HoldRoute::Input,
        kind: None,
        after_bytes: 0,
    });

    // Assert
    assert_eq!((input.len(), preview.len(), second.len()), (10, 10, 10));
    assert!(still_armed.armed && !still_armed.held);
    assert_eq!(hold.status().job_id, Some(job));
    assert!(!rearm, "a taken hold cannot be armed again");
    drop(taken);
}

#[tokio::test]
async fn a_hold_nobody_releases_lets_go_at_its_limit() {
    // Arrange
    let hold = Arc::new(WorkerHold::with_limit(Duration::from_millis(100)));
    assert!(hold.arm(HoldRequest {
        route: HoldRoute::Input,
        kind: None,
        after_bytes: 5,
    }));
    let (source, whole) = body(&[20]);

    // Act
    let read = tokio::time::timeout(
        BOUND,
        rest(hold.wrap(HoldRoute::Input, None, Uuid::from_u128(4), source)),
    )
    .await
    .expect("the stream ends without a release");

    // Assert
    assert_eq!(read, whole);
    assert_eq!(hold.status(), HoldStatus::default());
}

#[tokio::test]
async fn a_stream_dropped_while_it_is_held_lets_the_hold_go() {
    // Arrange: the supervisor is killed mid-fetch, so the server drops the body while it waits.
    let hold = armed(HoldRoute::Input, None, 5);
    let (source, _) = body(&[20]);
    let mut stream = hold.wrap(HoldRoute::Input, None, Uuid::from_u128(5), source);
    let first = next(&mut stream).await;
    let reading = tokio::spawn(async move { next(&mut stream).await });
    wait_held(&hold).await;

    // Act
    reading.abort();
    assert!(reading.await.is_err_and(|error| error.is_cancelled()));

    // Assert
    assert_eq!(first.map(|chunk| chunk.len()), Some(5));
    assert_eq!(hold.status(), HoldStatus::default());
    assert!(hold.arm(HoldRequest {
        route: HoldRoute::Input,
        kind: None,
        after_bytes: 1,
    }));
}

#[test]
fn a_stream_dropped_before_it_was_read_lets_the_hold_go() {
    // Arrange: the handler took the hold, then refused the request before it read the body.
    let hold = armed(HoldRoute::Output, Some("thumb"), 5);
    let (source, _) = body(&[20]);
    let stream = hold.wrap(HoldRoute::Output, Some("thumb"), Uuid::from_u128(6), source);
    assert!(hold.status().job_id.is_some(), "the hold is taken");

    // Act
    drop(stream);

    // Assert
    assert_eq!(hold.status(), HoldStatus::default());
}

#[test]
fn dropping_a_finished_stream_leaves_a_new_arm_in_place() {
    // Arrange
    let hold = armed(HoldRoute::Input, None, 5);
    let (source, _) = body(&[20]);
    let stream = hold.wrap(HoldRoute::Input, None, Uuid::from_u128(7), source);
    hold.release();
    assert!(hold.arm(HoldRequest {
        route: HoldRoute::Output,
        kind: None,
        after_bytes: 1,
    }));

    // Act
    drop(stream);

    // Assert
    assert!(
        hold.status().armed,
        "the stream of the old hold does not clear the new one"
    );
}

#[test]
fn an_input_hold_with_a_kind_is_refused() {
    // Act
    let request = hold_request(&json!({"route": "input", "kind": "thumb", "after_bytes": 5}));

    // Assert
    assert!(
        request.is_none(),
        "an input stream has no kind, so the hold could never be taken"
    );
}

#[test]
fn a_hold_request_names_a_route_an_optional_kind_and_its_bytes() {
    // Arrange
    let valid = json!({"route": "output", "kind": "thumb", "after_bytes": 7});
    let refused = [
        json!({"route": "lease", "after_bytes": 1}),
        json!({"route": "output", "kind": "../x", "after_bytes": 1}),
        json!({"route": "input", "after_bytes": -1}),
        json!({"route": "input"}),
    ];

    // Act
    let parsed = hold_request(&valid);

    // Assert
    assert_eq!(
        parsed,
        Some(HoldRequest {
            route: HoldRoute::Output,
            kind: Some("thumb".to_owned()),
            after_bytes: 7,
        })
    );
    for body in &refused {
        assert_eq!(hold_request(body), None, "{body}");
    }
}
