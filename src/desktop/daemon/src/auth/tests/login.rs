//! AC-3 and AC-4 of AZ-3908 (IT-14-23 at the unit tier): poll pacing, terminal states, `HttpsRequired`.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use ipc_schema::LoginState;
use sc_error::TransportKind;
use sync_core::{SyncApi, SyncError};
use sync_core_proto::DevicePoll;

use super::fake_api::{FakeApi, USER_CODE, VERIFICATION_URL, token};
use super::{ACCOUNT, DEVICE_TOKEN, harness};
use crate::auth::{LoginError, SignInServer};
use crate::ipc::tests::capture::Captured;

const HTTPS_SERVER: &str = "https://sync.example.test";
const INTERVAL_SECS: u64 = 5;

fn server(api: &Arc<FakeApi>, url: &str) -> SignInServer {
    SignInServer {
        url: url.to_owned(),
        api: Arc::clone(api) as Arc<dyn SyncApi>,
    }
}

#[tokio::test]
async fn login_polls_at_the_interval_adds_five_seconds_after_slow_down_and_stores_the_token() {
    // Arrange
    let h = harness();
    let captured = Captured::default();
    let _installed = captured.install();
    let api = Arc::new(
        FakeApi::new(Arc::clone(&h.clock), INTERVAL_SECS).answering(vec![
            Ok(DevicePoll::Pending),
            Ok(DevicePoll::SlowDown),
            Ok(DevicePoll::Pending),
            Ok(token(DEVICE_TOKEN)),
        ]),
    );

    // Act
    let started = h
        .session
        .login_start(Some(&server(&api, HTTPS_SERVER)), "Work laptop")
        .await
        .expect("login starts");
    let pending = h.session.login_status();
    let settled = h.settle_login(60).await;

    // Assert
    assert_eq!(started.verification_url, VERIFICATION_URL);
    assert_eq!(started.user_code, USER_CODE);
    assert!(!format!("{started:?}").contains(USER_CODE));
    assert_eq!(pending, LoginState::Pending);
    assert_eq!(settled, LoginState::Approved);
    let secs = |s| Duration::from_secs(s);
    assert_eq!(api.poll_gaps(), vec![secs(5), secs(5), secs(10), secs(10)]);
    assert_eq!(h.stored_token().as_deref(), Some(DEVICE_TOKEN));
    assert_eq!(h.browser.opened(), vec![VERIFICATION_URL.to_owned()]);
    let leaked = captured
        .events()
        .into_iter()
        .filter(|(_, text)| text.contains(DEVICE_TOKEN) || text.contains(USER_CODE))
        .count();
    assert_eq!(leaked, 0);
}

#[tokio::test]
async fn a_denied_grant_ends_denied_with_no_token() {
    // Arrange
    let h = harness();
    let api = Arc::new(
        FakeApi::new(Arc::clone(&h.clock), INTERVAL_SECS)
            .answering(vec![Ok(DevicePoll::Pending), Ok(DevicePoll::Denied)]),
    );

    // Act
    h.session
        .login_start(Some(&server(&api, HTTPS_SERVER)), "Work laptop")
        .await
        .expect("login starts");
    let settled = h.settle_login(30).await;

    // Assert
    assert_eq!(settled, LoginState::Denied);
    assert_eq!(h.stored_token(), None);
}

#[tokio::test]
async fn an_expired_grant_ends_expired_with_no_token() {
    // Arrange
    let h = harness();
    let api = Arc::new(
        FakeApi::new(Arc::clone(&h.clock), INTERVAL_SECS).answering(vec![Ok(DevicePoll::Expired)]),
    );

    // Act
    h.session
        .login_start(Some(&server(&api, HTTPS_SERVER)), "Work laptop")
        .await
        .expect("login starts");
    let settled = h.settle_login(30).await;

    // Assert
    assert_eq!(settled, LoginState::Expired);
    assert_eq!(h.stored_token(), None);
}

#[tokio::test]
async fn a_plain_http_server_is_https_required_before_any_request() {
    // Arrange
    let h = harness();
    let api = Arc::new(FakeApi::new(Arc::clone(&h.clock), INTERVAL_SECS));

    // Act
    let started = h
        .session
        .login_start(
            Some(&server(&api, "http://sync.example.test")),
            "Work laptop",
        )
        .await;

    // Assert
    assert_eq!(started, Err(LoginError::HttpsRequired));
    assert_eq!(api.requests(), 0);
    assert!(h.browser.opened().is_empty());
    assert_eq!(h.session.login_status(), LoginState::SignedOut);
}

#[tokio::test]
async fn login_without_a_server_is_server_not_set() {
    // Arrange
    let h = harness();

    // Act
    let started = h.session.login_start(None, "Work laptop").await;

    // Assert
    assert_eq!(started, Err(LoginError::ServerNotSet));
}

#[tokio::test]
async fn an_unreachable_server_is_server_unreachable_and_stays_signed_out() {
    // Arrange
    let h = harness();
    let api = Arc::new(
        FakeApi::new(Arc::clone(&h.clock), INTERVAL_SECS)
            .grant_failing(SyncError::Transport(TransportKind::Connect)),
    );

    // Act
    let started = h
        .session
        .login_start(Some(&server(&api, HTTPS_SERVER)), "Work laptop")
        .await;

    // Assert
    assert_eq!(started, Err(LoginError::ServerUnreachable));
    assert_eq!(h.session.login_status(), LoginState::SignedOut);
    assert!(h.browser.opened().is_empty());
}

#[tokio::test]
async fn a_poll_lost_in_transport_is_retried_at_the_interval() {
    // Arrange
    let h = harness();
    let api = Arc::new(
        FakeApi::new(Arc::clone(&h.clock), INTERVAL_SECS).answering(vec![
            Err(SyncError::Transport(TransportKind::Timeout)),
            Ok(token(DEVICE_TOKEN)),
        ]),
    );

    // Act
    h.session
        .login_start(Some(&server(&api, HTTPS_SERVER)), "Work laptop")
        .await
        .expect("login starts");
    let settled = h.settle_login(30).await;

    // Assert
    assert_eq!(settled, LoginState::Approved);
    assert_eq!(api.poll_gaps(), vec![Duration::from_secs(5); 2]);
    assert_eq!(h.stored_token().as_deref(), Some(DEVICE_TOKEN));
}

#[tokio::test]
async fn an_approved_token_the_store_refuses_ends_signed_out_and_is_not_written() {
    // Arrange
    let h = harness();
    let token_dir = h.dir.path().join("sunnycloud");
    std::fs::create_dir(&token_dir).expect("token dir");
    std::fs::set_permissions(&token_dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let api = Arc::new(
        FakeApi::new(Arc::clone(&h.clock), INTERVAL_SECS).answering(vec![Ok(token(DEVICE_TOKEN))]),
    );

    // Act
    h.session
        .login_start(Some(&server(&api, HTTPS_SERVER)), "Work laptop")
        .await
        .expect("login starts");
    let settled = h.settle_login(30).await;

    // Assert
    assert_eq!(settled, LoginState::SignedOut);
    assert!(!token_dir.join(format!("token-{ACCOUNT}")).exists());
}
