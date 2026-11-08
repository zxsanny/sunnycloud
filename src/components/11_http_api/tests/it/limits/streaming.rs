use sunnycloud_http_api::registry::BodyClass;

use super::support::*;

#[tokio::test]
async fn a_chunked_body_is_cut_at_the_limit_and_the_port_is_not_called() {
    // Arrange
    let server = Limited::start().await;
    let mut raw = Raw::connect(server.public.addr).await;
    let chunk = vec![b'a'; 40 * KIB];

    // Act
    raw.send(&Raw::head(
        "POST",
        path_of(BodyClass::Small),
        "transfer-encoding: chunked\r\n",
        "",
    ))
    .await;
    for _ in 0..2 {
        raw.send(format!("{:x}\r\n", chunk.len()).as_bytes()).await;
        raw.send(&chunk).await;
        raw.send(b"\r\n").await;
    }
    raw.send(b"0\r\n\r\n").await;
    let answer = raw.finish(ANSWER_BOUND).await;

    // Assert
    assert_eq!(status_of(&answer), Some(413));
    assert_eq!(body_of(&answer)["code"], "size_too_large");
    assert_eq!(server.port_calls(), 0);
    assert_eq!(server.limit_exceeded().await, 1.0);
    server.stop().await;
}

#[tokio::test]
async fn a_chunked_body_within_the_limit_reaches_the_port() {
    // Arrange
    let server = Limited::start().await;
    let mut raw = Raw::connect(server.public.addr).await;

    // Act
    raw.send(&Raw::head(
        "POST",
        path_of(BodyClass::Small),
        "transfer-encoding: chunked\r\n",
        "",
    ))
    .await;
    raw.send(b"5\r\nhello\r\n0\r\n\r\n").await;
    let answer = raw.finish(ANSWER_BOUND).await;

    // Assert
    assert_eq!(
        status_of(&answer),
        Some(202),
        "the unscripted port answers an error"
    );
    assert_eq!(server.port_calls(), 1);
    server.stop().await;
}
