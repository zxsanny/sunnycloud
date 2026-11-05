use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::UnixListener as StdListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ipc_schema::*;
use tokio::net::UnixStream;
use tracing::Level;

use super::capture::Captured;
use super::fake::{Client, FakeDaemon};
use super::scenarios;
use crate::ipc::unix::{check_private_dir, clear_leftover_socket, effective_uid};
use crate::ipc::{EndpointRefused, IpcServer, start_in};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A private runtime directory standing in for `$XDG_RUNTIME_DIR`. It sits directly under
/// `/tmp` because a socket path may not exceed 108 bytes, which deep temp dirs do.
struct Runtime(PathBuf);

impl Runtime {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/scd-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }

    fn dir(&self) -> PathBuf {
        self.0.join("sunnycloud")
    }

    fn socket(&self) -> PathBuf {
        self.dir().join("daemon.sock")
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("cannot remove {}: {error}", self.0.display());
        }
    }
}

fn my_uid() -> u32 {
    effective_uid()
}

fn make_dir(path: &Path, mode: u32) {
    DirBuilder::new().mode(mode).create(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

async fn start(runtime: &Runtime, daemon: &Arc<FakeDaemon>) -> Result<IpcServer, EndpointRefused> {
    start_in(Arc::clone(daemon), &runtime.0, my_uid()).await
}

async fn connect(path: &Path) -> Client<UnixStream> {
    Client::new(UnixStream::connect(path).await.unwrap())
}

#[tokio::test]
async fn endpoint_is_a_0600_socket_in_a_0700_directory_owned_by_the_user() {
    let runtime = Runtime::new();
    let daemon = FakeDaemon::new();

    let server = start(&runtime, &daemon).await.unwrap();

    let dir = fs::metadata(runtime.dir()).unwrap();
    let socket = fs::metadata(runtime.socket()).unwrap();
    assert_eq!(dir.mode() & 0o7777, 0o700);
    assert_eq!(socket.mode() & 0o7777, 0o600);
    assert_eq!((dir.uid(), socket.uid()), (my_uid(), my_uid()));
    assert_eq!(server.endpoint(), runtime.socket().to_str().unwrap());
    let mut client = connect(&runtime.socket()).await;
    assert!(matches!(
        client.call(MethodCall::Status(NoParams {})).await.outcome,
        Outcome::Success { .. }
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn full_session_over_the_real_socket_records_no_token() {
    let runtime = Runtime::new();
    let daemon = FakeDaemon::new();
    let server = start(&runtime, &daemon).await.unwrap();
    let mut client = connect(&runtime.socket()).await;

    scenarios::token_never_on_the_wire(&mut client, &daemon).await;

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_input_over_the_real_socket_leaves_other_connections_working() {
    let runtime = Runtime::new();
    let daemon = FakeDaemon::new();
    let server = start(&runtime, &daemon).await.unwrap();
    let mut a = connect(&runtime.socket()).await;
    let mut b = connect(&runtime.socket()).await;

    scenarios::malformed_input(&mut a, &mut b).await;

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn two_subscribers_on_the_real_socket_receive_both_events() {
    let runtime = Runtime::new();
    let daemon = FakeDaemon::new();
    let server = start(&runtime, &daemon).await.unwrap();
    let mut a = connect(&runtime.socket()).await;
    let mut b = connect(&runtime.socket()).await;

    scenarios::events_to_subscribers(&mut a, &mut b).await;

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_removes_the_socket_and_a_new_server_can_start() {
    let runtime = Runtime::new();
    let daemon = FakeDaemon::new();
    let server = start(&runtime, &daemon).await.unwrap();

    server.shutdown().await.unwrap();

    assert!(!runtime.socket().exists());
    start(&runtime, &daemon)
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn a_leftover_socket_of_a_killed_daemon_is_replaced() {
    let runtime = Runtime::new();
    make_dir(&runtime.dir(), 0o700);
    let crashed = StdListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    drop(crashed); // a killed process leaves the socket file behind
    assert!(runtime.socket().exists());
    let daemon = FakeDaemon::new();

    let server = start(&runtime, &daemon).await.unwrap();

    let mut client = connect(&runtime.socket()).await;
    assert!(matches!(
        client.call(MethodCall::Status(NoParams {})).await.outcome,
        Outcome::Success { .. }
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_second_daemon_on_a_live_endpoint_is_refused_and_the_first_keeps_serving() {
    let runtime = Runtime::new();
    let daemon = FakeDaemon::new();
    let first = start(&runtime, &daemon).await.unwrap();

    let second = start(&runtime, &daemon).await;

    assert!(matches!(second, Err(EndpointRefused::InUse { .. })));
    let mut client = connect(&runtime.socket()).await;
    assert!(matches!(
        client.call(MethodCall::Status(NoParams {})).await.outcome,
        Outcome::Success { .. }
    ));
    first.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_live_socket_of_another_process_is_refused_and_left_untouched() {
    let runtime = Runtime::new();
    make_dir(&runtime.dir(), 0o700);
    let squatter = StdListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = FakeDaemon::new();

    let refused = start(&runtime, &daemon).await;

    assert!(matches!(refused, Err(EndpointRefused::InUse { .. })));
    assert!(runtime.socket().exists());
    drop(squatter);
}

#[tokio::test]
async fn refusal_is_logged_as_error_and_the_daemon_stays_usable() {
    let runtime = Runtime::new();
    make_dir(&runtime.dir(), 0o755);
    let daemon = FakeDaemon::new();
    let captured = Captured::default();
    let _installed = captured.install();

    let refused = start(&runtime, &daemon).await;

    assert!(matches!(
        refused,
        Err(EndpointRefused::WrongMode { mode: 0o755, .. })
    ));
    let errors: Vec<String> = captured
        .events()
        .into_iter()
        .filter(|(level, _)| *level == Level::ERROR)
        .map(|(_, text)| text)
        .collect();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("endpoint refused"), "{errors:?}");
    assert!(
        errors[0].contains("0.755") || errors[0].contains("755"),
        "{errors:?}"
    );
    // The refusal is an ordinary result: the daemon (and so its sync) carries on.
    assert!(daemon.status().await.is_ok());
}

#[tokio::test]
async fn a_runtime_directory_of_another_user_is_refused() {
    let runtime = Runtime::new();
    let daemon = FakeDaemon::new();

    let refused = start_in(daemon, &runtime.0, my_uid() + 1).await;

    assert!(matches!(refused, Err(EndpointRefused::ForeignOwner { .. })));
}

#[tokio::test]
async fn a_daemon_directory_of_another_user_is_refused() {
    let runtime = Runtime::new();
    make_dir(&runtime.dir(), 0o700);

    let checked = check_private_dir(&runtime.dir(), my_uid() + 1);

    assert!(matches!(checked, Err(EndpointRefused::ForeignOwner { .. })));
}

#[tokio::test]
async fn a_socket_of_another_user_is_refused_and_not_removed() {
    let runtime = Runtime::new();
    make_dir(&runtime.dir(), 0o700);
    let squatter = StdListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o600)).unwrap();

    let checked = clear_leftover_socket(&runtime.socket(), my_uid() + 1).await;

    assert!(matches!(checked, Err(EndpointRefused::ForeignOwner { .. })));
    assert!(runtime.socket().exists());
    drop(squatter);
}

#[tokio::test]
async fn a_socket_with_the_wrong_mode_is_refused() {
    let runtime = Runtime::new();
    make_dir(&runtime.dir(), 0o700);
    let squatter = StdListener::bind(runtime.socket()).unwrap();
    fs::set_permissions(runtime.socket(), fs::Permissions::from_mode(0o666)).unwrap();
    let daemon = FakeDaemon::new();

    let refused = start(&runtime, &daemon).await;

    assert!(matches!(
        refused,
        Err(EndpointRefused::WrongMode { mode: 0o666, .. })
    ));
    drop(squatter);
}

#[tokio::test]
async fn a_regular_file_at_the_socket_path_is_refused() {
    let runtime = Runtime::new();
    make_dir(&runtime.dir(), 0o700);
    fs::write(runtime.socket(), b"x").unwrap();
    let daemon = FakeDaemon::new();

    let refused = start(&runtime, &daemon).await;

    assert!(matches!(refused, Err(EndpointRefused::NotASocket { .. })));
    assert!(runtime.socket().exists());
}

#[tokio::test]
async fn a_symlink_in_place_of_the_daemon_directory_is_refused() {
    let runtime = Runtime::new();
    let elsewhere = runtime.0.join("elsewhere");
    make_dir(&elsewhere, 0o700);
    symlink(&elsewhere, runtime.dir()).unwrap();
    let daemon = FakeDaemon::new();

    let refused = start(&runtime, &daemon).await;

    assert!(matches!(
        refused,
        Err(EndpointRefused::NotADirectory { .. })
    ));
}

#[tokio::test]
async fn a_runtime_directory_open_to_others_is_refused() {
    let runtime = Runtime::new();
    fs::set_permissions(&runtime.0, fs::Permissions::from_mode(0o755)).unwrap();
    let daemon = FakeDaemon::new();

    let refused = start(&runtime, &daemon).await;

    assert!(matches!(
        refused,
        Err(EndpointRefused::WrongMode { mode: 0o755, .. })
    ));
}
