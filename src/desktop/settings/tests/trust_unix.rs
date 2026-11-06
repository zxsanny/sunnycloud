//! AC-1 on Linux: an endpoint the current user does not own is refused before a byte is
//! sent. The squatters are real sockets that count what they receive.
#![cfg(unix)]

mod common;

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use sunnycloud_settings::daemon_client::{
    ClientConfig, ClientError, ConnectionState, DaemonClient,
};
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::task::JoinHandle;
use tracing::Level;

use common::{Captured, WAIT, config};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// `<root>/sunnycloud/daemon.sock` under a private root directly in `/tmp`, so the socket
/// path stays under the 108-byte limit.
struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let root = PathBuf::from(format!(
            "/tmp/scs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        make_dir(&root, 0o700);
        make_dir(&root.join("sunnycloud"), 0o700);
        Self { root }
    }

    fn dir(&self) -> PathBuf {
        self.root.join("sunnycloud")
    }

    fn socket(&self) -> PathBuf {
        self.dir().join("daemon.sock")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("cannot remove {}: {error}", self.root.display());
        }
    }
}

fn make_dir(path: &Path, mode: u32) {
    DirBuilder::new().mode(mode).create(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// A listener that counts the connections and the bytes it receives.
struct Squatter {
    connections: Arc<AtomicUsize>,
    bytes: Arc<AtomicUsize>,
    accept: JoinHandle<()>,
}

impl Squatter {
    fn listen(path: &Path, mode: u32) -> Self {
        let listener = UnixListener::bind(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let bytes = Arc::new(AtomicUsize::new(0));
        let accept = tokio::spawn({
            let connections = Arc::clone(&connections);
            let bytes = Arc::clone(&bytes);
            async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    connections.fetch_add(1, Ordering::SeqCst);
                    let bytes = Arc::clone(&bytes);
                    tokio::spawn(async move {
                        let mut chunk = [0_u8; 1024];
                        while let Ok(read) = stream.read(&mut chunk).await {
                            if read == 0 {
                                break;
                            }
                            bytes.fetch_add(read, Ordering::SeqCst);
                        }
                    });
                }
            }
        });
        Self {
            connections,
            bytes,
            accept,
        }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn bytes(&self) -> usize {
        self.bytes.load(Ordering::SeqCst)
    }
}

impl Drop for Squatter {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

fn my_uid() -> u32 {
    ClientConfig::new(PathBuf::new()).owner_uid
}

/// Runs the client until it has refused the endpoint twice, then checks every AC-1 effect.
async fn assert_refused(config: ClientConfig, squatter: &Squatter, reason: &str) {
    let captured = Captured::default();
    let _guard = captured.install();
    let (client, _events) = DaemonClient::spawn(config);
    let mut state = client.connection();

    let refused = tokio::time::timeout(
        WAIT,
        state.wait_for(|s| matches!(s, ConnectionState::Down { attempt, .. } if *attempt >= 2)),
    )
    .await
    .expect("the client refuses twice in time")
    .unwrap()
    .clone();
    let call = client.status().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let ConnectionState::Down { cause, .. } = refused else {
        unreachable!("the wait above accepts only Down");
    };
    match &cause {
        ClientError::EndpointUntrusted(text) => assert!(
            text.contains(reason),
            "reason {text:?} does not contain {reason:?}"
        ),
        other => panic!("expected EndpointUntrusted, got {other:?}"),
    }
    assert!(matches!(call, Err(ClientError::EndpointUntrusted(_))));
    assert_eq!(squatter.connections(), 0, "the squatter was connected to");
    assert_eq!(squatter.bytes(), 0, "the squatter received bytes");
    assert_eq!(
        captured.count(Level::WARN, "endpoint untrusted"),
        1,
        "one WARN per distinct refusal: {:?}",
        captured.events()
    );
}

#[tokio::test]
async fn a_socket_of_another_owner_is_refused_with_zero_bytes_sent() {
    let tree = Tree::new();
    let squatter = Squatter::listen(&tree.socket(), 0o600);
    let mut config = config(tree.socket().to_str().unwrap());
    // Another OS user cannot be created here; "the endpoint is owned by someone else" is
    // the same comparison with the expected owner shifted.
    config.owner_uid = my_uid() + 1;

    assert_refused(config, &squatter, "belongs to uid").await;
}

#[tokio::test]
async fn a_world_writable_socket_is_refused_with_zero_bytes_sent() {
    let tree = Tree::new();
    let squatter = Squatter::listen(&tree.socket(), 0o666);

    assert_refused(
        config(tree.socket().to_str().unwrap()),
        &squatter,
        "mode 0666",
    )
    .await;
}

#[tokio::test]
async fn the_right_owner_in_a_group_writable_directory_is_refused_with_zero_bytes_sent() {
    let tree = Tree::new();
    let squatter = Squatter::listen(&tree.socket(), 0o600);
    fs::set_permissions(tree.dir(), fs::Permissions::from_mode(0o770)).unwrap();

    assert_refused(
        config(tree.socket().to_str().unwrap()),
        &squatter,
        "mode 0770",
    )
    .await;
}

#[tokio::test]
async fn a_group_writable_runtime_directory_is_refused_with_zero_bytes_sent() {
    let tree = Tree::new();
    let squatter = Squatter::listen(&tree.socket(), 0o600);
    fs::set_permissions(&tree.root, fs::Permissions::from_mode(0o770)).unwrap();

    assert_refused(
        config(tree.socket().to_str().unwrap()),
        &squatter,
        "mode 0770",
    )
    .await;
    fs::set_permissions(&tree.root, fs::Permissions::from_mode(0o700)).unwrap();
}

#[tokio::test]
async fn a_symlink_to_a_good_socket_is_refused() {
    let tree = Tree::new();
    let target = tree.dir().join("real.sock");
    let squatter = Squatter::listen(&target, 0o600);
    symlink(&target, tree.socket()).unwrap();

    assert_refused(
        config(tree.socket().to_str().unwrap()),
        &squatter,
        "not a socket",
    )
    .await;
}

#[tokio::test]
async fn a_regular_file_at_the_endpoint_is_refused() {
    let tree = Tree::new();
    fs::write(tree.socket(), b"not a socket").unwrap();
    fs::set_permissions(tree.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let idle = Squatter::listen(&tree.dir().join("other.sock"), 0o600);

    assert_refused(
        config(tree.socket().to_str().unwrap()),
        &idle,
        "not a socket",
    )
    .await;
}

#[tokio::test]
async fn an_endpoint_of_the_current_user_with_the_documented_modes_is_trusted() {
    let tree = Tree::new();
    let listener = Squatter::listen(&tree.socket(), 0o600);
    let (client, _events) = DaemonClient::spawn(config(tree.socket().to_str().unwrap()));

    common::wait_connected(&client).await;

    assert_eq!(listener.connections(), 1);
    assert_eq!(listener.bytes(), 0, "nothing is sent until a call is made");
}
