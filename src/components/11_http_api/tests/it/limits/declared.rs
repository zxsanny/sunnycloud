use std::time::{Duration, Instant};

use sunnycloud_http_api::registry::BodyClass;

use super::support::*;

const FAST: Duration = Duration::from_millis(500);
const TEN_GIB: u64 = 10 * 1024 * 1024 * 1024;

fn declared(path: &str, length: u64) -> Vec<u8> {
    Raw::head("POST", path, &format!("content-length: {length}\r\n"), "")
}

#[tokio::test]
async fn a_declared_length_one_byte_over_the_class_limit_is_413_before_any_body_byte() {
    // Arrange
    let server = Limited::start().await;
    let classes = [
        (BodyClass::Small, 64 * KIB),
        (BodyClass::Commit, 4 * MIB),
        (BodyClass::Batch, 4 * MIB),
        (BodyClass::Block, 2 * MIB),
        (BodyClass::Staged, 2 * MIB),
        (BodyClass::Query, 128 * KIB),
        (BodyClass::Auth, 16 * KIB),
    ];

    // Act
    let mut answers = Vec::new();
    for (class, limit) in classes {
        let started = Instant::now();
        let mut raw = Raw::connect(server.public.addr).await;
        raw.send(&declared(path_of(class), limit as u64 + 1)).await;
        let answer = raw.finish(ANSWER_BOUND).await;
        answers.push((class, started.elapsed(), answer));
    }

    // Assert
    for (class, elapsed, answer) in &answers {
        assert_eq!(status_of(answer), Some(413), "{class:?}");
        assert_eq!(body_of(answer)["code"], "size_too_large", "{class:?}");
        assert!(*elapsed < FAST, "{class:?} answered after {elapsed:?}");
    }
    assert_eq!(server.limit_exceeded().await, classes.len() as f64);
    assert_eq!(server.port_calls(), 0);
    server.stop().await;
}

#[tokio::test]
async fn a_ten_gib_declaration_is_refused_without_a_byte_sent() {
    // Arrange
    let server = Limited::start().await;
    let mut raw = Raw::connect(server.public.addr).await;

    // Act
    raw.send(&declared(path_of(BodyClass::Commit), TEN_GIB))
        .await;
    let answer = raw.finish(ANSWER_BOUND).await;

    // Assert
    assert_eq!(status_of(&answer), Some(413));
    assert_eq!(server.limit_exceeded().await, 1.0);
    assert_eq!(server.port_calls(), 0);
    server.stop().await;
}

#[tokio::test]
async fn a_body_of_exactly_the_class_limit_reaches_the_port() {
    // Arrange
    let server = Limited::start().await;
    let cases = [(BodyClass::Small, 64 * KIB), (BodyClass::Block, 2 * MIB)];

    // Act
    let mut statuses = Vec::new();
    for (class, limit) in cases {
        let mut raw = Raw::connect(server.public.addr).await;
        raw.send(&declared(path_of(class), limit as u64)).await;
        raw.send(&vec![7u8; limit]).await;
        statuses.push(status_of(&raw.finish(ANSWER_BOUND).await));
    }

    // Assert
    assert_eq!(
        statuses,
        [Some(202), Some(202)],
        "the unscripted port answers an error"
    );
    assert_eq!(server.port_calls(), 2);
    assert_eq!(server.limit_exceeded().await, 0.0);
    server.stop().await;
}

#[tokio::test]
async fn a_get_with_a_declared_body_is_refused_and_one_without_length_is_ignored() {
    // Arrange
    let server = Limited::start().await;
    let path = path_of(BodyClass::None);
    let mut with_length = Raw::connect(server.public.addr).await;
    let mut chunked = Raw::connect(server.public.addr).await;

    // Act
    with_length
        .send(&Raw::head("GET", path, "content-length: 1048576\r\n", ""))
        .await;
    let refused = with_length.finish(ANSWER_BOUND).await;
    chunked
        .send(&Raw::head(
            "GET",
            path,
            "transfer-encoding: chunked\r\n",
            "",
        ))
        .await;
    chunked.send(b"4\r\nabcd\r\n0\r\n\r\n").await;
    let ignored = chunked.finish(ANSWER_BOUND).await;

    // Assert
    assert_eq!(status_of(&refused), Some(413));
    assert_eq!(status_of(&ignored), Some(202));
    assert_eq!(
        server.port_calls(),
        1,
        "only the chunked GET reached its handler"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_worker_output_is_capped_by_its_kind() {
    // Arrange
    let server = Limited::start().await;
    server.admit_workers(3);
    let put = |kind: &str, length: usize| {
        Raw::head(
            "PUT",
            &output_path(kind),
            &format!("content-length: {length}\r\n"),
            "authorization: Bearer worker\r\n",
        )
    };

    // Act
    let mut thumb = Raw::connect(server.internal.addr).await;
    thumb.send(&put("thumb", WORKER_THUMB_CAP + 1)).await;
    let thumb_answer = thumb.finish(ANSWER_BOUND).await;
    let mut preview = Raw::connect(server.internal.addr).await;
    preview.send(&put("preview", WORKER_THUMB_CAP + 1)).await;
    preview.send(&vec![1u8; WORKER_THUMB_CAP + 1]).await;
    let preview_answer = preview.finish(ANSWER_BOUND).await;
    let mut huge = Raw::connect(server.internal.addr).await;
    huge.send(&put("preview", WORKER_PREVIEW_CAP + 1)).await;
    let huge_answer = huge.finish(ANSWER_BOUND).await;

    // Assert
    assert_eq!(status_of(&thumb_answer), Some(413));
    assert_eq!(
        status_of(&preview_answer),
        Some(202),
        "the preview cap is larger"
    );
    assert_eq!(status_of(&huge_answer), Some(413));
    assert_eq!(server.limit_exceeded().await, 2.0);
    assert_eq!(server.port_calls(), 1);
    server.stop().await;
}

#[tokio::test]
async fn a_block_put_without_content_length_is_411_and_the_port_is_not_called() {
    // Arrange
    let server = Limited::start().await;
    let mut chunked = Raw::connect(server.public.addr).await;
    let mut bare = Raw::connect(server.public.addr).await;

    // Act
    chunked
        .send(&Raw::head(
            "POST",
            path_of(BodyClass::Block),
            "transfer-encoding: chunked\r\n",
            "",
        ))
        .await;
    chunked.send(b"5\r\nhello\r\n0\r\n\r\n").await;
    bare.send(&Raw::head("POST", path_of(BodyClass::Block), "", ""))
        .await;
    let answers = [
        chunked.finish(ANSWER_BOUND).await,
        bare.finish(ANSWER_BOUND).await,
    ];

    // Assert
    for answer in &answers {
        assert_eq!(status_of(answer), Some(411));
        assert_eq!(body_of(answer)["code"], "length_required");
    }
    assert_eq!(server.port_calls(), 0);
    server.stop().await;
}
