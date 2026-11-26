//! AZ-4381: the one-shot worker transfer hold of the test image, through the real `sunnycloud serve` binary
//! (`--features test-clock`) on PostgreSQL 18 and a blob volume. `POST /test/worker-hold` on `:9091` arms it; the
//! next matching worker input or output passes its bytes and waits for `DELETE /test/worker-hold`. The worker
//! suite (AZ-3878, IT-09-14) kills the supervisor while a transfer is held. The time limit of an unreleased hold
//! is proven in the crate's unit tests (`test_hold/tests.rs`), with a short limit.
#![cfg(feature = "test-clock")]

mod pipeline_support;
mod root_support;
mod serve_support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use pipeline_support::Pipeline;
use pipeline_support::media::{WEBP, WORKER_TOKEN, jpeg_bytes, problem_code};
use serde_json::{Value, json};
use serve_support::http::{Response, call, send};
use uuid::Uuid;

const HOLD: &str = "/test/worker-hold";
const BOUND: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(50);

fn hold_call(pipeline: &Pipeline, method: &str, body: &Value) -> Response {
    let headers = [("Content-Type", "application/json".to_owned())];
    let bytes = if body.is_null() {
        Vec::new()
    } else {
        body.to_string().into_bytes()
    };
    send(
        pipeline.stack.ports.test_clock,
        method,
        HOLD,
        &headers,
        &bytes,
    )
}

fn arm(pipeline: &Pipeline, request: Value) {
    let armed = hold_call(pipeline, "POST", &request);
    assert_eq!(armed.status, 204, "{armed:?}");
}

fn release(pipeline: &Pipeline) {
    let released = hold_call(pipeline, "DELETE", &Value::Null);
    assert_eq!(released.status, 204, "{released:?}");
}

fn status(pipeline: &Pipeline) -> Value {
    let answer = hold_call(pipeline, "GET", &Value::Null);
    assert_eq!(answer.status, 200, "{answer:?}");
    answer.json()
}

/// Polls `GET /test/worker-hold` until a stream is held; its status.
async fn wait_held(pipeline: &Pipeline) -> Value {
    let deadline = Instant::now() + BOUND;
    loop {
        let now = status(pipeline);
        if now["held"] == true {
            return now;
        }
        assert!(Instant::now() < deadline, "nothing held: {now}");
        tokio::time::sleep(POLL).await;
    }
}

/// `GET .../input` on its own thread, counting the body bytes as they arrive; the whole body.
fn read_input(addr: SocketAddr, job: Uuid, received: Arc<AtomicUsize>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("connect to the worker listener");
        stream.set_read_timeout(Some(BOUND)).expect("read timeout");
        let head = format!(
            "GET /api/v1/worker-jobs/{job}/input HTTP/1.1\r\nHost: api\r\nConnection: close\r\n\
             Authorization: Bearer {WORKER_TOKEN}\r\nX-Worker-Id: w1\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).expect("the request");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).expect("a head line");
            if line.trim_end().is_empty() {
                break;
            }
        }
        let mut body = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let read = reader.read(&mut chunk).expect("the body");
            if read == 0 {
                return body;
            }
            body.extend_from_slice(&chunk[..read]);
            received.fetch_add(read, Ordering::SeqCst);
        }
    })
}

/// `PUT .../outputs/{kind}` of the 1x1 WebP on its own thread; the status.
fn post_output(addr: SocketAddr, job: Uuid, kind: &'static str) -> JoinHandle<u16> {
    thread::spawn(move || {
        let headers = [
            ("Content-Type", "image/webp".to_owned()),
            ("X-Worker-Id", "w1".to_owned()),
            ("X-Width", "1".to_owned()),
            ("X-Height", "1".to_owned()),
        ];
        let path = format!("/api/v1/worker-jobs/{job}/outputs/{kind}");
        call(addr, "PUT", &path, WORKER_TOKEN, &headers, &WEBP).status
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn ac1_an_input_is_held_after_its_bytes_and_the_release_delivers_the_rest() {
    // Arrange: a 300 KB photo uploaded through 03, its job claimed, a hold on the input after 100,000 bytes.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("hold1");
    let bytes = jpeg_bytes(300_000, 7);
    pipeline.commit_file(folder, "photo.jpg", &bytes);
    let job = pipeline.claim_kind("w1", "image", "stills");
    arm(&pipeline, json!({"route": "input", "after_bytes": 100_000}));
    let received = Arc::new(AtomicUsize::new(0));

    // Act
    let reader = read_input(pipeline.stack.ports.internal, job, Arc::clone(&received));
    let held = wait_held(&pipeline).await;
    let deadline = Instant::now() + BOUND;
    while received.load(Ordering::SeqCst) < 100_000 && Instant::now() < deadline {
        tokio::time::sleep(POLL).await;
    }
    let before_release = received.load(Ordering::SeqCst);
    release(&pipeline);
    let body = reader.join().expect("the reader");

    // Assert
    assert_eq!(
        held,
        json!({"armed": false, "held": true, "job_id": job, "bytes": 100_000})
    );
    assert_eq!(
        before_release, 100_000,
        "the stream did not stop at its bytes"
    );
    assert!(body == bytes, "the released input is not the uploaded file");
    assert_eq!(status(&pipeline)["held"], false);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac2_an_output_is_held_while_the_job_waits_and_the_release_accepts_it() {
    // Arrange: a stills job and a hold on its thumb after 10 bytes.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("hold2");
    pipeline.commit_file(folder, "photo.jpg", &jpeg_bytes(30_000, 1));
    let job = pipeline.claim_kind("w1", "image", "stills");
    arm(
        &pipeline,
        json!({"route": "output", "kind": "thumb", "after_bytes": 10}),
    );

    // Act
    let poster = post_output(pipeline.stack.ports.internal, job, "thumb");
    let held = wait_held(&pipeline).await;
    let (while_held, _) = pipeline.job(job).await.expect("the job");
    let rows_while_held = pipeline.derived_rows().await.len();
    release(&pipeline);
    let answer = poster.join().expect("the poster");

    // Assert
    assert_eq!(
        (held["job_id"].clone(), held["bytes"].clone()),
        (json!(job), json!(10))
    );
    assert_eq!(while_held.status, "running");
    assert_eq!(rows_while_held, 0, "an output was stored while held");
    assert_eq!(answer, 204);
    assert_eq!(pipeline.derived_rows().await.len(), 1);
    pipeline.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ac3_the_hold_takes_one_matching_transfer_and_a_second_arm_waits_for_it() {
    // Arrange: two stills jobs; a hold on a thumb, from its first byte.
    let pipeline = Pipeline::start(&[]).await;
    let folder = pipeline.folder("hold3");
    pipeline.commit_file(folder, "a.jpg", &jpeg_bytes(30_000, 1));
    pipeline.commit_file(folder, "b.jpg", &jpeg_bytes(30_000, 2));
    let first = pipeline.claim_kind("w1", "image", "stills");
    let second = pipeline.claim_kind("w1", "image", "stills");
    arm(
        &pipeline,
        json!({"route": "output", "kind": "thumb", "after_bytes": 0}),
    );
    let addr = pipeline.stack.ports.internal;

    // Act
    let preview = post_output(addr, first, "preview")
        .join()
        .expect("the preview");
    let after_preview = status(&pipeline);
    let thumb = post_output(addr, first, "thumb");
    let held = wait_held(&pipeline).await;
    let second_arm = hold_call(
        &pipeline,
        "POST",
        &json!({"route": "input", "after_bytes": 0}),
    );
    release(&pipeline);
    let thumb = thumb.join().expect("the thumb");
    let other = post_output(addr, second, "thumb")
        .join()
        .expect("the other thumb");

    // Assert
    assert_eq!(preview, 204, "a preview is not the armed kind");
    assert_eq!(
        (
            after_preview["armed"].clone(),
            after_preview["held"].clone()
        ),
        (json!(true), json!(false))
    );
    assert_eq!(held["job_id"], json!(first));
    assert_eq!(
        problem_code(&second_arm),
        (400, "invalid_request".into()),
        "{second_arm:?}"
    );
    let refusal = String::from_utf8_lossy(&second_arm.body);
    assert!(refusal.contains("already_armed"), "{refusal}");
    assert_eq!((thumb, other), (204, 204));
    assert_eq!(
        status(&pipeline),
        json!({"armed": false, "held": false, "job_id": null, "bytes": 0})
    );
    pipeline.finish().await;
}
