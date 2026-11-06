//! AZ-3871 AC-2: every hostile reply of the decoder is a typed violation and a dropped connection.
//! The peer is a fake decoder on a real Unix socket; the memory bound is in `decoder_memory.rs`.

mod decoder_support;

use std::future::pending;
use std::time::Duration;

use decoder_support::{HostilePeer, eventually, frame, hostile_cases, same_kind};
use media_shared::decoder_protocol::{DecodeError, Event, ProtocolViolation, ViolationKind};
use serde_json::json;
use sunnycloud_worker::{CallError, DecoderConn};

const WAIT: Duration = Duration::from_secs(5);

fn socket() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temporary folder");
    let path = dir.path().join("decoder.sock");
    (dir, path)
}

#[tokio::test]
async fn every_hostile_probe_reply_is_a_typed_violation_and_drops_the_connection() {
    for (name, reply, expected) in hostile_cases() {
        // Arrange
        let (_dir, path) = socket();
        let peer = HostilePeer::start(&path, reply);
        let mut conn = DecoderConn::connect(&path).await.expect("connect");

        // Act
        let result = conn.probe("/scratch/job", &mut |_| {}, pending()).await;

        // Assert
        match result {
            Err(CallError::Violation(ProtocolViolation(kind))) => {
                assert!(same_kind(kind, expected), "{name}: got {kind:?}");
            }
            other => panic!("{name}: expected a violation, got {other:?}"),
        }
        assert!(!conn.is_open(), "{name}: the connection is still open");
        assert!(
            matches!(conn.hello().await, Err(CallError::Disconnected)),
            "{name}: a later call used the dropped connection"
        );
        assert!(
            eventually(WAIT, || peer.closed() == 1).await,
            "{name}: the peer never saw the hang-up"
        );
    }
}

#[tokio::test]
async fn a_violation_after_valid_events_still_delivers_the_events_and_drops_the_connection() {
    // Arrange
    let (_dir, path) = socket();
    let mut reply = frame(json!({"event": "alive"}).to_string().as_bytes());
    reply.extend(frame(
        json!({"event": {"progress": {"out_time_ms": 5, "speed": 1.0}}})
            .to_string()
            .as_bytes(),
    ));
    reply.extend(frame(b"garbage"));
    let _peer = HostilePeer::start(&path, reply);
    let mut conn = DecoderConn::connect(&path).await.expect("connect");
    let mut seen = Vec::new();

    // Act
    let result = conn
        .probe("/scratch/job", &mut |event| seen.push(event), pending())
        .await;

    // Assert
    assert!(matches!(
        result,
        Err(CallError::Violation(ProtocolViolation(ViolationKind::Json)))
    ));
    assert_eq!(
        seen,
        vec![
            Event::Alive,
            Event::Progress {
                out_time_ms: 5,
                speed: 1.0
            }
        ]
    );
    assert!(!conn.is_open());
}

#[tokio::test]
async fn an_event_answering_hello_is_a_violation() {
    // Arrange
    let (_dir, path) = socket();
    let _peer = HostilePeer::start(
        &path,
        frame(json!({"event": "alive"}).to_string().as_bytes()),
    );
    let mut conn = DecoderConn::connect(&path).await.expect("connect");

    // Act
    let result = conn.hello().await;

    // Assert
    assert!(matches!(
        result,
        Err(CallError::Violation(ProtocolViolation(
            ViolationKind::OutOfSequence
        )))
    ));
    assert!(!conn.is_open());
}

#[tokio::test]
async fn capabilities_for_another_protocol_are_a_mismatch_and_drop_the_connection() {
    // Arrange
    let (_dir, path) = socket();
    let capabilities = json!({"response": {"capabilities": {
        "protocol": 2, "vips": "v", "ffmpeg": "f", "libheif": "h",
        "build_timestamp": 1, "nvenc_probe_ok": false, "nvenc_sessions": 0}}});
    let _peer = HostilePeer::start(&path, frame(capabilities.to_string().as_bytes()));
    let mut conn = DecoderConn::connect(&path).await.expect("connect");

    // Act
    let result = conn.hello().await;

    // Assert
    assert!(matches!(
        result,
        Err(CallError::Decode(DecodeError::ProtocolMismatch))
    ));
    assert!(!conn.is_open());
}

#[tokio::test]
async fn a_peer_that_closes_before_or_inside_a_frame_is_a_closed_connection_not_a_hang() {
    let cases: [(&str, Vec<u8>); 3] = [
        ("closes at once", Vec::new()),
        ("closes inside the prefix", vec![0, 0]),
        ("closes inside the payload", {
            let mut bytes = 100u32.to_be_bytes().to_vec();
            bytes.extend_from_slice(b"{\"resp");
            bytes
        }),
    ];
    for (name, reply) in cases {
        // Arrange
        let (_dir, path) = socket();
        let _peer = HostilePeer::start_hanging_up(&path, reply);
        let mut conn = DecoderConn::connect(&path).await.expect("connect");

        // Act
        let result = conn.probe("/scratch/job", &mut |_| {}, pending()).await;

        // Assert
        assert!(
            matches!(result, Err(CallError::Closed)),
            "{name}: {result:?}"
        );
        assert!(!conn.is_open(), "{name}");
    }
}

#[tokio::test]
async fn a_frame_that_announces_a_length_and_stalls_leaves_the_caller_in_control() {
    // Arrange
    let (_dir, path) = socket();
    let mut reply = 100u32.to_be_bytes().to_vec();
    reply.extend_from_slice(b"{\"resp");
    let _peer = HostilePeer::start(&path, reply);
    let mut conn = DecoderConn::connect(&path).await.expect("connect");

    // Act
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        conn.probe("/scratch/job", &mut |_| {}, pending()),
    )
    .await;

    // Assert
    assert!(
        result.is_err(),
        "the stalled frame ended the call by itself"
    );
    assert!(
        !conn.is_open(),
        "an abandoned call must close the connection"
    );
}

#[tokio::test]
async fn an_out_of_range_request_is_refused_before_anything_is_sent() {
    // Arrange
    let (_dir, path) = socket();
    let peer = HostilePeer::start(&path, Vec::new());
    let mut conn = DecoderConn::connect(&path).await.expect("connect");

    // Act
    let result = conn.probe("", &mut |_| {}, pending()).await;

    // Assert
    assert!(matches!(result, Err(CallError::InvalidRequest(_))));
    assert!(conn.is_open());
    assert_eq!(peer.closed(), 0);
}
