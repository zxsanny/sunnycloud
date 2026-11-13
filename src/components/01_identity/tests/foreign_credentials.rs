//! ST-01-4 (AC-79, NFT-SEC-50), AZ-3692 AC-3: no credential other than a session access token or a device token
//! opens a principal. The worker bearer is the configured `worker_api_token`; the refresh token, the mobile login
//! code and the device code are minted by the real services. Each is presented to `authenticate` as a session and
//! as a device credential, and no row of any table changes.

mod support;

use contracts::{Authenticator, AuthenticatorError, ClientKind, DeviceGrantService};
use support::auth::{authenticator, device, session};
use support::device::{geo, grants, ip};
use support::invites::invites;
use support::load_settings;
use support::onboarding::{all_rows, first_admin};
use support::registration::Rig;
use support::sign_in::CHALLENGE;

const POOL: u32 = 5;
const SUBJECT: &str = "identity-1";

#[tokio::test]
async fn worker_bearer_refresh_token_login_code_and_device_code_give_no_principal_and_touch_no_row()
{
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let service = grants(&mut rig.world.harness, geo(&[]));
    let signed_in = first_admin(&rig, &operator, SUBJECT).await;
    let login_code = rig
        .world
        .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
        .await;
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let worker_bearer = load_settings(&[]).worker_api_token.expose().clone();
    let foreign = [
        ("worker bearer", worker_bearer.as_str()),
        ("refresh token", signed_in.refresh.expose()),
        ("login code", login_code.expose()),
        ("device code", authorization.device_code.expose()),
    ];
    let authenticator = authenticator(&rig.world.harness);
    let rows_before = all_rows(&mut rig.world.conn, true).await;

    // Act
    let mut outcomes = Vec::new();
    for (name, value) in foreign {
        for credential in [session(value), device(value)] {
            let result = authenticator.authenticate(credential).await;
            outcomes.push((name, result));
        }
    }

    // Assert
    let refused = outcomes
        .iter()
        .filter(|(_, result)| matches!(result, Err(AuthenticatorError::Unauthenticated)))
        .count();
    assert_eq!(refused, 8, "{outcomes:?}");
    assert_eq!(all_rows(&mut rig.world.conn, true).await, rows_before);
    drop((authenticator, service, operator));
    rig.finish().await;
}
