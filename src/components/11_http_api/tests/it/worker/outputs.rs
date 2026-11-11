use contracts::{AcceptOutcome, WorkerJobError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::support::{JOB, Stack, WORKER, as_worker, job_path, worker_bearer};

const THUMB: [(&str, &str); 4] = [
    ("content-type", "image/webp"),
    ("x-width", "320"),
    ("x-height", "240"),
    ("x-duration-ms", "1500"),
];

#[tokio::test]
async fn ac3_an_output_passes_kind_type_dimensions_length_and_body_and_answers_204_or_200() {
    // Arrange
    let stack = Stack::start().await;
    stack.jobs.push(
        "WorkerJobs::accept_output",
        Ok::<_, WorkerJobError>(AcceptOutcome::Accepted),
    );
    stack.jobs.push(
        "WorkerJobs::accept_output",
        Ok::<_, WorkerJobError>(AcceptOutcome::AlreadyPresent),
    );

    // Act
    let accepted = as_worker(
        stack.internal,
        "PUT",
        &job_path("outputs/thumb"),
        &THUMB,
        b"webp-bytes",
    )
    .await;
    let present = as_worker(
        stack.internal,
        "PUT",
        &job_path("outputs/preview"),
        &[("content-type", "image/jpeg")],
        b"jpeg",
    )
    .await;

    // Assert
    assert_eq!(accepted.status, 204);
    assert!(accepted.body.is_empty());
    assert_eq!(accepted.header("cache-control"), Some("no-store"));
    assert_eq!(present.status, 200);
    assert_eq!(
        stack.jobs.calls(),
        [
            format!(
                "accept_output {JOB} {WORKER} Thumb OutputMeta {{ media_type: \"image/webp\", width: Some(320), height: Some(240), duration_ms: Some(1500) }} length=10 body=webp-bytes"
            ),
            format!(
                "accept_output {JOB} {WORKER} Preview OutputMeta {{ media_type: \"image/jpeg\", width: None, height: None, duration_ms: None }} length=4 body=jpeg"
            ),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn ac3_the_output_errors_of_08_map_to_their_rows_of_the_status_table() {
    // Arrange
    let stack = Stack::start().await;
    let errors = [
        WorkerJobError::LeaseLost,
        WorkerJobError::RevisionObsolete,
        WorkerJobError::OutputTooLarge,
        WorkerJobError::OutputTypeRejected,
        WorkerJobError::OutputKindRejected,
    ];
    for error in errors {
        stack
            .jobs
            .push("WorkerJobs::accept_output", Err::<AcceptOutcome, _>(error));
    }

    // Act
    let mut answers = Vec::new();
    for _ in 0..5 {
        let answer = as_worker(
            stack.internal,
            "PUT",
            &job_path("outputs/thumb"),
            &THUMB,
            b"x",
        )
        .await;
        answers.push((answer.status, answer.code()));
    }

    // Assert
    let expected = [
        (409, "lease_lost"),
        (404, "revision_obsolete"),
        (413, "output_too_large"),
        (422, "output_type_rejected"),
        (422, "output_kind_rejected"),
    ];
    assert_eq!(
        answers,
        expected.map(|(status, code)| (status, code.to_owned()))
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_unknown_kind_is_422_a_bad_header_is_400_and_neither_reaches_the_port() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let unknown = as_worker(
        stack.internal,
        "PUT",
        &job_path("outputs/poster"),
        &THUMB,
        b"x",
    )
    .await;
    let no_type = as_worker(stack.internal, "PUT", &job_path("outputs/thumb"), &[], b"x").await;
    let bad_width = as_worker(
        stack.internal,
        "PUT",
        &job_path("outputs/thumb"),
        &[("content-type", "image/webp"), ("x-width", "wide")],
        b"x",
    )
    .await;

    // Assert
    assert_eq!(
        (unknown.status, unknown.code().as_str()),
        (422, "output_kind_rejected")
    );
    assert_eq!(
        (no_type.status, no_type.code().as_str()),
        (400, "invalid_request")
    );
    assert_eq!(
        (bad_width.status, bad_width.code().as_str()),
        (400, "invalid_request")
    );
    assert!(stack.jobs.calls().is_empty(), "{:?}", stack.jobs.calls());
    stack.stop().await;
}

#[tokio::test]
async fn a_declared_length_over_the_cap_of_its_kind_is_413_before_the_port_is_called() {
    // Arrange
    let stack = Stack::start().await;
    let thumb_cap = 5 << 20;
    let over = vec![0u8; thumb_cap + 1];

    // Act
    let thumb = as_worker(
        stack.internal,
        "PUT",
        &job_path("outputs/thumb"),
        &THUMB,
        &over,
    )
    .await;

    // Assert
    assert_eq!(
        (thumb.status, thumb.code().as_str()),
        (413, "size_too_large")
    );
    assert!(stack.jobs.calls().is_empty(), "{:?}", stack.jobs.calls());
    stack.stop().await;
}

#[tokio::test]
async fn an_output_without_content_length_is_411_and_never_reaches_the_port() {
    // Arrange
    let stack = Stack::start().await;
    let request = format!(
        "PUT {} HTTP/1.1\r\nhost: sunnycloud.test\r\nauthorization: {}\r\nx-worker-id: {WORKER}\r\ncontent-type: image/webp\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        job_path("outputs/thumb"),
        worker_bearer()
    );

    // Act
    let mut stream = TcpStream::connect(stack.internal)
        .await
        .expect("tcp connect");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("request is sent");
    let mut text = String::new();
    stream
        .read_to_string(&mut text)
        .await
        .expect("answer is read");

    // Assert
    assert!(text.starts_with("HTTP/1.1 411"), "{text}");
    assert!(text.contains("length_required"), "{text}");
    assert!(stack.jobs.calls().is_empty(), "{:?}", stack.jobs.calls());
    stack.stop().await;
}
