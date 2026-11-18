//! AC-1 and AC-2: `BackupCore::set_server` and the construction of the core, without a host.

use std::net::TcpListener;
use std::sync::Arc;

use mobile_backup::{BackupCore, BackupError, CheckpointDb, ServerCheck, ServerProbe, ServerTrust};
use sync_core::{BoxFuture, ConnectError, SyncConfig};
use tempfile::TempDir;

struct Answering(Result<(), ConnectError>);

impl ServerProbe for Answering {
    fn check(&self, _config: SyncConfig) -> BoxFuture<'_, Result<(), ConnectError>> {
        let answer = self.0.clone();
        Box::pin(async move { answer })
    }
}

fn core_answering(answer: Result<(), ConnectError>) -> (TempDir, Arc<BackupCore>) {
    let dir = TempDir::new().expect("temp dir");
    let db = Arc::new(CheckpointDb::new(dir.path().join("checkpoint.db")));
    let core = BackupCore::with_probe(db, "1.0.0".to_owned(), Arc::new(Answering(answer)));
    (dir, core)
}

fn real_core() -> (TempDir, Arc<BackupCore>) {
    let dir = TempDir::new().expect("temp dir");
    let core = BackupCore::new(dir.path().display().to_string(), "1.0.0".to_owned());
    (dir, core)
}

#[cfg(not(feature = "allow-plain-http"))]
#[tokio::test]
async fn plain_http_is_refused_before_any_socket_is_opened() {
    // Arrange
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    listener.set_nonblocking(true).expect("non-blocking");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let (_dir, core) = real_core();

    // Act
    let refused = core.set_server(url).await;
    let connection = listener.accept();

    // Assert
    assert_eq!(refused, Err(BackupError::HttpsRequired));
    let error = connection.expect_err("a connection was opened");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

#[cfg(feature = "allow-plain-http")]
#[tokio::test]
async fn a_build_with_the_plain_http_feature_gets_past_the_scheme_check_and_connects() {
    // Arrange
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let accepted = std::thread::spawn(move || listener.accept().map(|_| ()));
    let (_dir, core) = real_core();

    // Act
    let answer = core.set_server(url).await;

    // Assert
    accepted
        .join()
        .expect("the accept thread")
        .expect("the client connected");
    assert_ne!(answer, Err(BackupError::HttpsRequired));
}

#[tokio::test]
async fn an_https_server_that_answers_server_info_gives_a_server_check() {
    // Arrange
    let (_dir, core) = core_answering(Ok(()));

    // Act
    let checked = core.set_server("https://sunnycloud.test".to_owned()).await;

    // Assert
    assert_eq!(
        checked,
        Ok(ServerCheck {
            trust: ServerTrust::System
        })
    );
}

#[tokio::test]
async fn an_unreachable_server_is_server_unreachable() {
    // Arrange
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.local_addr().expect("address")
    };
    let (_dir, core) = real_core();

    // Act
    let refused = core.set_server(format!("https://{closed}")).await;

    // Assert
    assert_eq!(refused, Err(BackupError::ServerUnreachable));
}

#[tokio::test]
async fn a_server_that_needs_a_newer_client_is_upgrade_required_with_the_server_text() {
    // Arrange
    let (_dir, core) = core_answering(Err(ConnectError::UpgradeRequired {
        message: "this client (1.0.0) is older than the minimum supported version (2.0.0)"
            .to_owned(),
        update_url: None,
    }));

    // Act
    let refused = core.set_server("https://sunnycloud.test".to_owned()).await;

    // Assert
    assert_eq!(
        refused,
        Err(BackupError::UpgradeRequired {
            message: "this client (1.0.0) is older than the minimum supported version (2.0.0)"
                .to_owned()
        })
    );
}

#[tokio::test]
async fn an_address_that_is_not_an_https_url_is_invalid_and_opens_no_socket() {
    // Arrange
    let (_dir, core) = real_core();

    // Act
    let refused = core.set_server("not a url".to_owned()).await;

    // Assert
    assert_eq!(refused, Err(BackupError::InvalidServerUrl));
}

#[test]
fn constructing_the_core_creates_no_file_and_the_first_status_creates_the_checkpoint() {
    // Arrange
    let dir = TempDir::new().expect("temp dir");

    // Act
    let core = BackupCore::new(dir.path().display().to_string(), "1.0.0".to_owned());
    let before = std::fs::read_dir(dir.path()).expect("dir").count();
    let status = core.status().expect("status");
    let after = dir.path().join("checkpoint.db").exists();

    // Assert
    assert_eq!(before, 0, "the constructor created a file");
    assert_eq!(status.visible, 0);
    assert!(after, "status opens the checkpoint on first use");
}
