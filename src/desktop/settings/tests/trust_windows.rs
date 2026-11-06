//! AC-1 on Windows: the pipe server SID check. Compile-checked on Linux only; these tests
//! have not run on a Windows host.
#![cfg(windows)]

mod common;

use std::time::Duration;

use sunnycloud_settings::daemon_client::{ClientError, ConnectionState, DaemonClient};
use sunnycloud_settings_testkit::script::Script;
use sunnycloud_settings_testkit::stub::StubDaemon;

use common::{WAIT, config, wait_connected};

#[tokio::test]
async fn a_pipe_served_by_the_current_user_is_trusted_and_answers() {
    let pipe = format!(r"\\.\pipe\sunnycloud-test-trusted-{}", std::process::id());
    let _stub = StubDaemon::start_on_pipe(Script::default(), pipe.clone())
        .await
        .unwrap();
    let (client, _events) = DaemonClient::spawn(config(&pipe));

    wait_connected(&client).await;

    assert!(client.status().await.is_ok());
}

#[tokio::test]
async fn the_default_endpoint_names_the_pipe_of_the_current_user() {
    let config = sunnycloud_settings::daemon_client::ClientConfig::for_current_user().unwrap();

    let name = config.endpoint.to_string_lossy().into_owned();

    assert!(
        name.starts_with(r"\\.\pipe\sunnycloud-daemon-S-1-"),
        "{name}"
    );
}

/// Needs a second Windows user: the harness runs `SC_FOREIGN_PIPE` (a pipe name) as that
/// user and a squatter listens there. Run with `--ignored` on a prepared host.
#[tokio::test]
#[ignore = "needs a pipe created by another Windows user (SC_FOREIGN_PIPE); no second user on the dev host"]
async fn a_pipe_served_by_another_user_is_refused() {
    let pipe = std::env::var("SC_FOREIGN_PIPE").expect("SC_FOREIGN_PIPE names the foreign pipe");
    let (client, _events) = DaemonClient::spawn(config(&pipe));
    let mut state = client.connection();

    let refused = tokio::time::timeout(
        WAIT,
        state.wait_for(|s| matches!(s, ConnectionState::Down { .. })),
    )
    .await
    .unwrap()
    .unwrap()
    .clone();

    assert!(matches!(
        refused,
        ConnectionState::Down {
            cause: ClientError::EndpointUntrusted(_),
            ..
        }
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
}
