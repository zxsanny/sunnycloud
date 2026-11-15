//! AZ-3791 AC-5 and AC-6 against the real `sunnycloud serve` binary: with 20 held long-polls and 10 uploads in
//! progress, SIGTERM runs the shutdown in its documented order and exits 0 within the grace (3 s) plus 2 s; after
//! a restart each upload asks only for the blocks that were never acknowledged (NFT-RES-22). SIGINT runs the same
//! order and exits 130.

mod root_support;
mod serve_support;

use std::net::SocketAddr;
use std::thread;
use std::time::{Duration, Instant};

use sc_ids::{BLOCK_SIZE, hash_block};
use serde_json::json;
use serve_support::account::Account;
use serve_support::http::{Response, call};
use serve_support::{Exited, STOP_STEPS, Stack, positions};
use uuid::Uuid;

const WITHIN: Duration = Duration::from_secs(10);
const LONG_POLLS: usize = 20;
const UPLOADS: usize = 10;
const BLOCKS: usize = 3;
/// `shutdown_grace` of the stack plus 2 s.
const EXIT_BOUND: Duration = Duration::from_secs(5);

fn assert_stop_order(exited: &Exited) {
    let messages = exited.messages();
    let ready = positions(&messages, &["serve start: ready"])[0];
    let at = positions(&messages, &STOP_STEPS);
    assert!(ready < at[0], "{messages:#?}");
    assert!(
        at.windows(2).all(|pair| pair[0] < pair[1]),
        "out of order: {messages:#?}"
    );
}

/// An upload of three blocks; blocks `0..acked` are sent and acknowledged.
struct Upload {
    id: Uuid,
    acked: usize,
}

fn block(upload: usize, index: usize) -> Vec<u8> {
    vec![u8::try_from(upload * BLOCKS + index + 1).expect("small"); BLOCK_SIZE]
}

fn start_upload(public: SocketAddr, account: &Account, number: usize) -> Uuid {
    let size = BLOCKS * BLOCK_SIZE;
    let body = json!({
        "parent_id": account.root,
        "name": format!("upload-{number}.bin"),
        "declared_size": size,
        "client_key": format!("serve-stop-{number}"),
    });
    let answer = call(
        public,
        "POST",
        "/api/v1/uploads",
        &account.token,
        &[("Content-Type", "application/json".to_owned())],
        body.to_string().as_bytes(),
    );
    assert!(answer.status < 300, "{answer:?}");
    let started = answer.json();
    assert_eq!(started["outcome"], "session", "{started}");
    started["upload_id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("upload id")
}

fn put_block(public: SocketAddr, account: &Account, upload: Uuid, number: usize, index: usize) {
    let bytes = block(number, index);
    let answer = call(
        public,
        "PUT",
        &format!("/api/v1/uploads/{upload}/blocks/{index}"),
        &account.token,
        &[
            ("Content-Type", "application/octet-stream".to_owned()),
            ("X-Blake3", hash_block(&bytes).to_string()),
        ],
        &bytes,
    );
    assert!(answer.status < 300, "{answer:?}");
}

/// Long-polls held from now on; each returns its answer and when it arrived.
fn hold_long_polls(
    public: SocketAddr,
    account: &Account,
) -> Vec<thread::JoinHandle<(Response, Instant)>> {
    let head = call(
        public,
        "GET",
        "/api/v1/changes/head",
        &account.token,
        &[],
        &[],
    );
    assert_eq!(head.status, 200, "{head:?}");
    let cursor = head.json()["cursor"].as_str().expect("cursor").to_owned();
    (0..LONG_POLLS)
        .map(|_| {
            let (token, path) = (
                account.token.clone(),
                format!("/api/v1/changes?cursor={cursor}&wait=25"),
            );
            thread::spawn(move || {
                let answer = call(public, "GET", &path, &token, &[], &[]);
                (answer, Instant::now())
            })
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn sigterm_runs_the_shutdown_in_order_and_a_restart_resumes_only_the_unacknowledged_blocks() {
    // Arrange: 10 uploads with 1 or 2 of 3 blocks acknowledged, 20 long-polls held.
    let stack = Stack::new().await;
    let account = stack.account().await;
    let mut server = stack.spawn();
    server.wait_ready(stack.ports.ops, WITHIN);
    let public = stack.ports.public;
    let uploads: Vec<Upload> = (0..UPLOADS)
        .map(|number| {
            let id = start_upload(public, &account, number);
            let acked = 1 + number % 2;
            for index in 0..acked {
                put_block(public, &account, id, number, index);
            }
            Upload { id, acked }
        })
        .collect();
    let polls = hold_long_polls(public, &account);
    thread::sleep(Duration::from_millis(500));

    // Act
    let signalled = server.signal("TERM");
    let answers: Vec<(Response, Instant)> = polls
        .into_iter()
        .map(|poll| poll.join().expect("long-poll thread"))
        .collect();
    let exited = server.wait_exit(WITHIN);
    let exited_after = exited.at.duration_since(signalled);
    let mut restarted = stack.spawn();
    restarted.wait_ready(stack.ports.ops, WITHIN);
    let need: Vec<serde_json::Value> = uploads
        .iter()
        .map(|upload| {
            let state = call(
                public,
                "GET",
                &format!("/api/v1/uploads/{}", upload.id),
                &account.token,
                &[],
                &[],
            );
            assert_eq!(state.status, 200, "{state:?}");
            state.json()["need_blocks"].clone()
        })
        .collect();
    restarted.signal("TERM");
    let second = restarted.wait_exit(WITHIN);

    // Assert
    assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
    assert!(exited_after < EXIT_BOUND, "{exited_after:?}");
    assert_stop_order(&exited);
    for (answer, at) in &answers {
        assert_eq!(answer.status, 200, "{answer:?}");
        assert!(*at >= signalled, "a long-poll returned before the signal");
        assert_eq!(answer.json()["items"], json!([]), "{answer:?}");
    }
    for (upload, need) in uploads.iter().zip(&need) {
        let expected: Vec<usize> = (upload.acked..BLOCKS).collect();
        assert_eq!(need, &json!(expected), "upload {}", upload.id);
    }
    assert_eq!(second.code, Some(0), "{}", second.stderr_text());
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sigint_runs_the_same_shutdown_order_and_exits_130() {
    // Arrange
    let stack = Stack::new().await;
    let mut server = stack.spawn();
    server.wait_ready(stack.ports.ops, WITHIN);

    // Act
    let signalled = server.signal("INT");
    let exited = server.wait_exit(WITHIN);

    // Assert
    assert_eq!(exited.code, Some(130), "{}", exited.stderr_text());
    assert!(exited.at.duration_since(signalled) < EXIT_BOUND);
    assert_stop_order(&exited);
    stack.finish().await;
}

/// The pool closes after the final group sync, as the last documented shutdown step.
#[tokio::test(flavor = "multi_thread")]
async fn the_pool_closes_last() {
    // Arrange
    let stack = Stack::new().await;
    let mut server = stack.spawn();
    server.wait_ready(stack.ports.ops, WITHIN);

    // Act
    server.signal("TERM");
    let exited = server.wait_exit(WITHIN);

    // Assert
    let messages = exited.messages();
    let at = positions(
        &messages,
        &["serve stop: final group sync", "serve stop: pool closed"],
    );
    assert!(at[0] < at[1], "{messages:#?}");
    stack.finish().await;
}
