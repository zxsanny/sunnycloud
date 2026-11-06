//! AC-5 of AZ-3970: fast and isolated start; plus the counters pacing and lane tests assert on.

mod support;

use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::json;
use support::{Answer, Client, Fixture, Route, Script};

async fn connect(fixture: &Fixture) -> Client {
    let config = fixture.client_config(&[&rustls::version::TLS13], &[b"h2"]);
    Client::connect(fixture.addr(), "127.0.0.1", config)
        .await
        .expect("handshake with the fixture")
}

async fn one_fixture(index: usize) -> (Duration, Vec<support::LoggedRequest>, String, u16) {
    let started = Instant::now();
    let script = Script::new().always(
        Route::ServerInfo,
        Answer::json(200, &json!({ "id": index })),
    );
    let fixture = Fixture::start(script).await;
    let startup = started.elapsed();
    let mut client = connect(&fixture).await;
    let reply = client
        .get(&format!("/api/v1/server-info?who={index}"))
        .await
        .expect("answer");
    (
        startup,
        fixture.requests(),
        String::from_utf8_lossy(&reply.body).into_owned(),
        fixture.addr().port(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_fixtures_start_fast_on_their_own_ports_and_share_nothing() {
    let tasks: Vec<_> = (0..50).map(|i| tokio::spawn(one_fixture(i))).collect();
    let mut ports = Vec::new();

    for (index, task) in tasks.into_iter().enumerate() {
        let (startup, log, body, port) = task.await.expect("fixture task");

        assert!(
            startup < Duration::from_secs(1),
            "fixture {index} started in {startup:?}"
        );
        assert_eq!(body, format!("{{\"id\":{index}}}"));
        assert_eq!(log.len(), 1, "fixture {index} saw only its own request");
        assert_eq!(log[0].target, format!("/api/v1/server-info?who={index}"));
        ports.push(port);
    }
    ports.sort_unstable();
    ports.dedup();
    assert_eq!(ports.len(), 50, "every fixture has its own port");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_puts_in_flight_are_counted_at_their_peak() {
    let hold = Answer::no_content().delay(Duration::from_millis(300));
    let script = Script::new().on_times(Route::PutBlock, hold, 4);
    let fixture = Fixture::start(script).await;
    let mut tasks = Vec::new();
    for index in 0..4 {
        let mut client = connect(&fixture).await;
        tasks.push(tokio::spawn(async move {
            client
                .send(
                    "PUT",
                    &format!("/api/v1/uploads/u1/blocks/{index}"),
                    &[],
                    Bytes::from_static(b"block"),
                )
                .await
        }));
    }

    for task in tasks {
        assert_eq!(task.await.expect("task").expect("ack").status, 204);
    }

    assert_eq!(fixture.with_stats(|s| s.max_block_puts_in_flight), 4);
    assert_eq!(fixture.with_stats(|s| s.block_puts_in_flight), 0);
}

#[tokio::test]
async fn staged_uploads_stay_uncommitted_until_a_batch_takes_them() {
    let key_a = "0190c5f0-0000-7000-8000-00000000000a";
    let key_b = "0190c5f0-0000-7000-8000-00000000000b";
    let script = Script::new()
        .always(Route::StagedUpload, Answer::json(200, &json!({})))
        .on(
            Route::BatchCommit,
            Answer::json(200, &json!({ "entries": [] })),
        );
    let fixture = Fixture::start(script).await;
    let mut client = connect(&fixture).await;
    for key in [key_a, key_b] {
        client
            .send(
                "PUT",
                &format!("/api/v1/staged-uploads/{key}"),
                &[],
                Bytes::from_static(b"x"),
            )
            .await
            .expect("staged");
    }
    assert_eq!(fixture.with_stats(|s| s.staged_uncommitted.len()), 2);

    let batch = json!({ "commit_id": "c", "entries": [{ "staged_key": key_a }] }).to_string();
    client
        .send("POST", "/api/v1/commits:batch", &[], Bytes::from(batch))
        .await
        .expect("batch");

    assert_eq!(
        fixture.with_stats(|s| s.staged_uncommitted.iter().cloned().collect::<Vec<_>>()),
        [key_b]
    );
    assert_eq!(fixture.with_stats(|s| s.max_staged_uncommitted), 2);
}
