//! AC-2 of AZ-3970: faults happen at the stated point, and the same script gives the same log.

mod support;

use std::time::{Duration, Instant};

use bytes::Bytes;
use support::{
    Answer, AppliedFault, Client, Failure, Fault, Fixture, LoggedRequest, Route, Script,
    seeded_bytes,
};
use sync_core_proto::BLOCK_SIZE;

const BLOCK: usize = BLOCK_SIZE;
const CONTENT_PATH: &str = "/api/v1/revisions/r1/content";

async fn connect(fixture: &Fixture) -> Client {
    let config = fixture.client_config(&[&rustls::version::TLS13], &[b"h2"]);
    Client::connect(fixture.addr(), "127.0.0.1", config)
        .await
        .expect("handshake with the fixture")
}

fn range(from: usize, to_inclusive: usize) -> String {
    format!("bytes={from}-{to_inclusive}")
}

async fn put_block(client: &mut Client, index: u32) -> Result<u16, Failure> {
    let path = format!("/api/v1/uploads/u1/blocks/{index}");
    let body = Bytes::from(vec![index as u8; 64]);
    client
        .send("PUT", &path, &[("x-blake3", "00")], body)
        .await
        .map(|reply| reply.status)
}

/// Drop after block 37: 37 acknowledged blocks, then the connection dies on block 37's request.
async fn run_drop_after_block_37() -> (Vec<LoggedRequest>, Result<u16, Failure>) {
    let script = Script::new()
        .on_times(Route::PutBlock, Answer::no_content(), 37)
        .on(
            Route::PutBlock,
            Answer::no_content().fault(Fault::DropConnection),
        );
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;
    for index in 0..37 {
        assert_eq!(put_block(&mut client, index).await.expect("ack"), 204);
    }
    let dropped = put_block(&mut client, 37).await;
    (fixture.requests(), dropped)
}

#[tokio::test]
async fn drop_after_block_37_is_exact_and_repeatable() {
    let (first, dropped) = run_drop_after_block_37().await;
    let (second, _) = run_drop_after_block_37().await;

    assert!(dropped.is_err(), "the 38th request gets no answer");
    assert_eq!(first.len(), 38);
    let last = first.last().expect("a last request");
    assert_eq!(last.block_index(), Some(37));
    assert_eq!(last.body.len(), 64, "the dropped request was read in full");
    assert_eq!(
        last.fault,
        Some(AppliedFault {
            kind: "drop_connection",
            at_byte: 0
        })
    );
    assert!(
        first[..37]
            .iter()
            .all(|r| r.answered == Some(204) && r.fault.is_none())
    );
    assert_eq!(first, second);
}

fn flip_script(content: &Bytes) -> Script {
    let offset = 7 * BLOCK + 123;
    Script::new().always(
        Route::RevisionContent,
        Answer::content(content.clone()).flip_bit(offset, 5),
    )
}

async fn download_with_flip() -> (Bytes, Vec<LoggedRequest>) {
    let content = seeded_bytes(42, 8 * BLOCK);
    let fixture = Fixture::start(flip_script(&content)).await;
    let mut client = connect(&fixture).await;
    let reply = client.get(CONTENT_PATH).await.expect("download");
    (reply.body, fixture.requests())
}

#[tokio::test]
async fn one_bit_of_block_7_is_flipped_and_nothing_else() {
    let original = seeded_bytes(42, 8 * BLOCK);

    let (received, log) = download_with_flip().await;
    let (again, log_again) = download_with_flip().await;

    let differing: Vec<usize> = (0..original.len())
        .filter(|&i| original[i] != received[i])
        .collect();
    assert_eq!(differing, [7 * BLOCK + 123]);
    assert_eq!(original[differing[0]] ^ received[differing[0]], 1 << 5);
    assert_eq!(received, again);
    assert_eq!(log, log_again);
}

#[tokio::test]
async fn flip_outside_the_requested_range_leaves_the_slice_intact() {
    let content = seeded_bytes(42, 8 * BLOCK);
    let fixture = Fixture::start(flip_script(&content)).await;
    let mut client = connect(&fixture).await;

    let block_3 = client
        .send(
            "GET",
            CONTENT_PATH,
            &[("range", &range(3 * BLOCK, 4 * BLOCK - 1))],
            Bytes::new(),
        )
        .await
        .expect("block 3");
    let block_7 = client
        .send(
            "GET",
            CONTENT_PATH,
            &[("range", &range(7 * BLOCK, 8 * BLOCK - 1))],
            Bytes::new(),
        )
        .await
        .expect("block 7");

    assert_eq!(block_3.body, content.slice(3 * BLOCK..4 * BLOCK));
    assert_ne!(block_7.body, content.slice(7 * BLOCK..8 * BLOCK));
}

async fn reset_after_full_block_3() -> (Failure, Vec<LoggedRequest>) {
    let content = seeded_bytes(42, 8 * BLOCK);
    let script = Script::new().on(
        Route::RevisionContent,
        Answer::content(content).fault(Fault::ResetAfterBytes(BLOCK)),
    );
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;
    let outcome = client
        .send(
            "GET",
            CONTENT_PATH,
            &[("range", &range(3 * BLOCK, 4 * BLOCK - 1))],
            Bytes::new(),
        )
        .await;
    (
        outcome.expect_err("the stream is reset"),
        fixture.requests(),
    )
}

#[tokio::test]
async fn reset_after_the_full_body_of_block_3() {
    let (failure, log) = reset_after_full_block_3().await;
    let (_, log_again) = reset_after_full_block_3().await;

    assert_eq!(failure.status, Some(206));
    assert!(failure.received <= BLOCK, "received {}", failure.received);
    assert_eq!(
        log[0].fault,
        Some(AppliedFault {
            kind: "reset_after_bytes",
            at_byte: BLOCK
        })
    );
    assert_eq!(log, log_again);
}

async fn close_at_3_mib_of_10() -> (Failure, Vec<LoggedRequest>) {
    let mib = 1024 * 1024;
    let script = Script::new().on(
        Route::RevisionContent,
        Answer::content(seeded_bytes(9, 10 * mib)).fault(Fault::ResetAfterBytes(3 * mib)),
    );
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;
    let failure = client
        .get(CONTENT_PATH)
        .await
        .expect_err("the stream is closed early");
    (failure, fixture.requests())
}

#[tokio::test]
async fn stream_closes_at_3_mib_of_10() {
    let mib = 1024 * 1024;

    let (failure, log) = close_at_3_mib_of_10().await;
    let (_, log_again) = close_at_3_mib_of_10().await;

    assert_eq!(failure.status, Some(200));
    assert!(failure.received <= 3 * mib, "received {}", failure.received);
    assert_eq!(
        log[0].fault,
        Some(AppliedFault {
            kind: "reset_after_bytes",
            at_byte: 3 * mib
        })
    );
    assert_eq!(log, log_again);
}

#[tokio::test]
async fn connection_drops_after_n_response_bytes() {
    let script = Script::new().on(
        Route::RevisionContent,
        Answer::content(seeded_bytes(1, 100_000)).fault(Fault::DropAfterBytes(40_000)),
    );
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;

    let failure = client
        .get(CONTENT_PATH)
        .await
        .expect_err("connection dropped");

    assert!(failure.received <= 40_000, "received {}", failure.received);
    assert_eq!(
        fixture.requests()[0].fault,
        Some(AppliedFault {
            kind: "drop_after_bytes",
            at_byte: 40_000
        })
    );
}

#[tokio::test]
async fn reset_stream_leaves_the_connection_usable() {
    let script = Script::new()
        .on(
            Route::PutBlock,
            Answer::no_content().fault(Fault::ResetStream),
        )
        .on(Route::PutBlock, Answer::no_content());
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;

    let reset = put_block(&mut client, 0).await;
    let retry = put_block(&mut client, 0).await;

    assert!(reset.is_err());
    assert_eq!(retry.expect("second attempt"), 204);
    assert_eq!(fixture.requests()[0].answered, None);
}

#[tokio::test]
async fn delay_holds_the_answer_for_the_scripted_time() {
    let delay = Duration::from_millis(300);
    let script = Script::new().on(Route::ServerInfo, Answer::no_content().delay(delay));
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;

    let started = Instant::now();
    let reply = client.get("/api/v1/server-info").await.expect("answer");

    assert_eq!(reply.status, 204);
    assert!(
        started.elapsed() >= delay,
        "answered after {:?}",
        started.elapsed()
    );
}
