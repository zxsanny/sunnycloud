//! The RFC 8628 polling half of the device login: one poll per server interval, the interval grown after each
//! `slow_down`, and one terminal state.

use std::sync::Arc;
use std::time::Duration;

use ipc_schema::LoginState;
use sync_core::{SyncApi, SyncError};
use sync_core_proto::DevicePoll;

use super::session::Shared;
use super::token_store::Token;

/// Plain `http://` only in a developer build (AC-54).
pub(super) fn allow_plain_http() -> bool {
    cfg!(feature = "allow-plain-http")
}

/// Polls until the grant is approved, denied or expired. The token goes to the store only; a store failure
/// ends signed out, since the server mints the token once.
pub(super) async fn poll_grant(
    shared: Arc<Shared>,
    api: Arc<dyn SyncApi>,
    device_code: String,
    mut interval: Duration,
) {
    loop {
        let Some(at) = shared.clock.monotonic().checked_add(interval) else {
            tracing::error!("the poll interval is out of range; the login attempt ends");
            shared.set(LoginState::SignedOut);
            return;
        };
        if shared.clock.sleep_until_monotonic(at).await.is_err() {
            tracing::info!("shutdown during a login attempt");
            return;
        }
        match api.device_grant_poll(&device_code).await {
            Ok(DevicePoll::Pending) => {}
            Ok(DevicePoll::SlowDown) => {
                interval = interval.saturating_add(shared.timing.slow_down_step)
            }
            Ok(DevicePoll::Denied) => return finish(&shared, LoginState::Denied),
            Ok(DevicePoll::Expired) => return finish(&shared, LoginState::Expired),
            Ok(DevicePoll::Token(token)) => {
                let state = match shared.save(Token::new(token.access_token)).await {
                    Ok(()) => LoginState::Approved,
                    Err(error) => {
                        tracing::error!(%error, "the device token could not be stored; sign in again");
                        LoginState::SignedOut
                    }
                };
                return finish(&shared, state);
            }
            Err(SyncError::Transport(kind)) => {
                tracing::warn!(?kind, "device login poll failed; retrying at the interval");
            }
            Err(SyncError::RateLimited { .. }) => {
                tracing::warn!("device login poll rate limited; retrying at the interval");
            }
            Err(error) => {
                tracing::error!(%error, "device login poll failed; the login attempt ends");
                return finish(&shared, LoginState::SignedOut);
            }
        }
    }
}

fn finish(shared: &Shared, state: LoginState) {
    if state == LoginState::Approved {
        tracing::info!("login approved");
    }
    shared.set(state);
}
