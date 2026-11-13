//! AC-4 of AZ-4231: sync-core's real client runs a device login against the real device routes, with 01's
//! device-grant port as a recording stub, and gets the token with no shape mismatch.

use std::sync::Arc;

use contracts::{DeviceGrantError, DeviceToken as GrantedToken};
use sc_config::Secret;
use sc_ids::TokenKind;
use sync_core::{
    Credential, CredentialProvider, CredentialUnavailable, HttpSyncClient, PersistFailed, SyncApi,
};
use sync_core_proto::{DevicePoll, DeviceToken, TokenType};

use super::support::{Stack, device_grant, token, user_code_text};
use super::tls_terminator::{Terminator, client_config};

const REQUEST: &str = "DeviceGrantService::request";
const POLL: &str = "DeviceGrantService::poll";

/// A daemon before its first sign-in: no token is stored, and the device-grant calls need none.
struct SignedOut;

impl CredentialProvider for SignedOut {
    fn bearer(&self) -> Result<Credential, CredentialUnavailable> {
        Err(CredentialUnavailable)
    }

    fn on_rotated(
        &self,
        _access: Secret<String>,
        _refresh: Secret<String>,
    ) -> Result<(), PersistFailed> {
        Err(PersistFailed)
    }
}

#[tokio::test]
async fn sync_core_runs_a_device_login_against_the_real_routes_and_gets_the_token() {
    // Arrange
    let stack = Stack::start().await;
    let grant = device_grant();
    let user_code = user_code_text(&grant);
    stack.script.push(REQUEST, Ok::<_, DeviceGrantError>(grant));
    let refusals = [
        DeviceGrantError::AuthorizationPending,
        DeviceGrantError::SlowDown,
        DeviceGrantError::AccessDenied,
        DeviceGrantError::ExpiredToken,
    ];
    for refusal in refusals {
        stack
            .script
            .push(POLL, Err::<GrantedToken, DeviceGrantError>(refusal));
    }
    stack.script.push(
        POLL,
        Ok::<_, DeviceGrantError>(GrantedToken(token(
            TokenKind::DeviceToken,
            "sc_dev_NEW_token",
        ))),
    );
    let terminator = Terminator::start(stack.public).await;
    let (api, _) = HttpSyncClient::connect_with_roots(
        client_config(format!("https://{}", terminator.addr), "desktop"),
        Arc::new(SignedOut),
        vec![terminator.ca.clone()],
    )
    .await
    .expect("the client connects through the terminator");

    // Act
    let started = api.device_grant_request("Studio PC", "linux").await;
    let mut polls = Vec::new();
    for _ in 0..5 {
        polls.push(api.device_grant_poll("dc-DEVICE_code").await);
    }

    // Assert
    let started = started.expect("the grant answer decodes");
    assert_eq!(started.device_code, "dc-DEVICE_code");
    assert_eq!(started.user_code, user_code);
    assert_eq!(started.verification_uri, "https://sunnycloud.test/device");
    assert_eq!((started.interval, started.expires_in), (5, 600));
    assert_eq!(
        polls,
        vec![
            Ok(DevicePoll::Pending),
            Ok(DevicePoll::SlowDown),
            Ok(DevicePoll::Denied),
            Ok(DevicePoll::Expired),
            Ok(DevicePoll::Token(DeviceToken {
                access_token: "sc_dev_NEW_token".to_owned(),
                token_type: TokenType::Bearer,
            })),
        ]
    );
    assert_eq!(
        stack.log(POLL),
        ["DeviceGrantService::poll dc-DEVICE_code"; 5]
    );
    drop(terminator);
    stack.stop().await;
}
