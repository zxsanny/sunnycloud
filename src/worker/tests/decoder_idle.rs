//! IT-09-15 at the client: between jobs the connection is checked without blocking, so a runner that exited or was
//! restarted while the slot was idle closes it before the next claim. A plain socket stands in for the runner, so
//! each end of the connection happens exactly when the test says.

use std::io::Write;
use std::os::unix::net::UnixListener;

use media_shared::decoder_protocol::ViolationKind;
use sunnycloud_worker::{CallError, DecoderConn};
use tempfile::TempDir;

struct Peer {
    _dir: TempDir,
    conn: DecoderConn,
    server: std::os::unix::net::UnixStream,
}

async fn connected() -> Peer {
    let dir = tempfile::tempdir().expect("socket folder");
    let socket = dir.path().join("decoder.sock");
    let listener = UnixListener::bind(&socket).expect("listener");
    let conn = DecoderConn::connect(&socket).await.expect("connect");
    let (server, _) = listener.accept().expect("accept");
    Peer {
        _dir: dir,
        conn,
        server,
    }
}

#[tokio::test]
async fn an_idle_connection_whose_runner_is_still_there_stays_open() {
    // Arrange
    let mut peer = connected().await;

    // Act
    let checked = peer.conn.check_idle();

    // Assert
    assert!(checked.is_ok(), "{checked:?}");
    assert!(peer.conn.is_open());
}

#[tokio::test]
async fn an_idle_connection_whose_runner_went_away_is_closed() {
    // Arrange
    let mut peer = connected().await;
    drop(peer.server);

    // Act
    let checked = peer.conn.check_idle();

    // Assert
    assert!(matches!(checked, Err(CallError::Closed)), "{checked:?}");
    assert!(!peer.conn.is_open());
}

#[tokio::test]
async fn a_byte_from_the_runner_while_no_job_runs_closes_the_connection_as_out_of_sequence() {
    // Arrange
    let mut peer = connected().await;
    peer.server.write_all(&[0]).expect("one byte");

    // Act
    let checked = peer.conn.check_idle();

    // Assert
    assert!(
        matches!(&checked, Err(CallError::Violation(v)) if v.0 == ViolationKind::OutOfSequence),
        "{checked:?}"
    );
    assert!(!peer.conn.is_open());
}

#[tokio::test]
async fn a_closed_connection_answers_disconnected() {
    // Arrange
    let mut peer = connected().await;
    drop(peer.server);
    assert!(peer.conn.check_idle().is_err(), "the first check closes it");

    // Act
    let checked = peer.conn.check_idle();

    // Assert
    assert!(
        matches!(checked, Err(CallError::Disconnected)),
        "{checked:?}"
    );
}
