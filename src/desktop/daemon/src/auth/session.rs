use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ipc_schema::{LoginState, LogoutOutcome};
use sc_clock::Clock;
use sync_core::{SyncApi, SyncError};
use sync_core_proto::{ServerUrl, UrlError};
use thiserror::Error;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::browser::BrowserLauncher;
use super::login::{allow_plain_http, poll_grant};
use super::token_store::{TokenStore, TokenStoreError};

/// Timing of the device login (RFC 8628 §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginTiming {
    /// Added to the poll interval after each `slow_down`, for that and every later poll.
    pub slow_down_step: Duration,
}

impl Default for LoginTiming {
    fn default() -> Self {
        Self {
            slow_down_step: Duration::from_secs(5),
        }
    }
}

/// The server the daemon is set to, with its connected client.
pub struct SignInServer {
    pub url: String,
    pub api: Arc<dyn SyncApi>,
}

/// What the user needs to approve the device. Never a token; `Debug` hides the user code.
#[derive(Clone, PartialEq, Eq)]
pub struct LoginStarted {
    pub verification_url: String,
    pub user_code: String,
}

impl fmt::Debug for LoginStarted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginStarted")
            .field("verification_url", &self.verification_url)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoginError {
    #[error("no server address is set")]
    ServerNotSet,
    #[error("the server address must use https")]
    HttpsRequired,
    #[error("the server address is not a valid URL")]
    InvalidServerUrl,
    #[error("the server is unreachable")]
    ServerUnreachable,
    #[error("the server refused the request: {0}")]
    Server(SyncError),
    #[error("token store: {0}")]
    Store(TokenStoreError),
    #[error("internal error")]
    Internal,
}

/// Everything a session needs; `clock` paces the polls.
pub struct SessionDeps {
    pub store: Arc<dyn TokenStore>,
    pub account: Uuid,
    pub browser: Arc<dyn BrowserLauncher>,
    pub clock: Arc<dyn Clock>,
    pub timing: LoginTiming,
}

pub(super) struct Shared {
    pub(super) store: Arc<dyn TokenStore>,
    pub(super) account: Uuid,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) timing: LoginTiming,
    pub(super) state: watch::Sender<LoginState>,
}

impl Shared {
    pub(super) fn set(&self, state: LoginState) {
        self.state.send_replace(state);
    }

    pub(super) async fn save(&self, token: super::token_store::Token) -> Result<(), LoginError> {
        let store = Arc::clone(&self.store);
        let account = self.account;
        blocking(move || store.save(account, &token)).await
    }
}

/// The sign-in state of the one account: device login, logout, and the move to signed-out when the server
/// rejects the token. Sync loops run their work through [`Session::while_signed_in`].
pub struct Session {
    shared: Arc<Shared>,
    browser: Arc<dyn BrowserLauncher>,
    attempt: Mutex<Option<JoinHandle<()>>>,
}

impl Session {
    /// Starts signed out; [`restore`](Self::restore) reads the stored token.
    pub fn new(deps: SessionDeps) -> Self {
        let (state, _) = watch::channel(LoginState::SignedOut);
        Self {
            shared: Arc::new(Shared {
                store: deps.store,
                account: deps.account,
                clock: deps.clock,
                timing: deps.timing,
                state,
            }),
            browser: deps.browser,
            attempt: Mutex::new(None),
        }
    }

    /// `Approved` when a token is stored, else `SignedOut`.
    pub async fn restore(&self) -> Result<LoginState, LoginError> {
        let store = Arc::clone(&self.shared.store);
        let account = self.shared.account;
        let stored = blocking(move || store.load(account)).await?;
        let state = if stored.is_some() {
            LoginState::Approved
        } else {
            LoginState::SignedOut
        };
        self.shared.set(state);
        Ok(state)
    }

    pub fn login_status(&self) -> LoginState {
        *self.shared.state.borrow()
    }

    /// Requests a device grant, opens the browser on the verification URL and polls in the background.
    /// An `http://` server is refused before any request unless the build has `allow-plain-http`.
    pub async fn login_start(
        &self,
        server: Option<&SignInServer>,
        device_name: &str,
    ) -> Result<LoginStarted, LoginError> {
        let server = server.ok_or(LoginError::ServerNotSet)?;
        ServerUrl::parse(&server.url, allow_plain_http()).map_err(|error| match error {
            UrlError::InsecureScheme => LoginError::HttpsRequired,
            UrlError::Invalid => LoginError::InvalidServerUrl,
        })?;
        self.abort_attempt();
        let grant = server
            .api
            .device_grant_request(device_name, std::env::consts::OS)
            .await
            .map_err(|error| match error {
                SyncError::Transport(_) => LoginError::ServerUnreachable,
                other => LoginError::Server(other),
            })?;
        self.open_browser(&grant.verification_uri);
        self.shared.set(LoginState::Pending);
        let task = tokio::spawn(poll_grant(
            Arc::clone(&self.shared),
            Arc::clone(&server.api),
            grant.device_code,
            Duration::from_secs(grant.interval),
        ));
        *self.attempt_slot() = Some(task);
        Ok(LoginStarted {
            verification_url: grant.verification_uri,
            user_code: grant.user_code,
        })
    }

    /// Revokes the token on the server (`DELETE /api/v1/devices/me`), then deletes the local copy even when the
    /// server could not be reached; `server_revoked = false` then says the server copy still needs revoking.
    pub async fn logout(&self, api: Option<Arc<dyn SyncApi>>) -> Result<LogoutOutcome, LoginError> {
        self.abort_attempt();
        let server_revoked = match api {
            Some(api) => match api.sign_out().await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "the device token could not be revoked on the server");
                    false
                }
            },
            None => false,
        };
        let store = Arc::clone(&self.shared.store);
        let account = self.shared.account;
        blocking(move || store.delete(account)).await?;
        self.shared.set(LoginState::SignedOut);
        Ok(LogoutOutcome { server_revoked })
    }

    /// Every sync loop reports its errors here. `Unauthorized` (the token was revoked or is gone) signs out;
    /// anything else, an unreachable server or identity provider included, changes nothing.
    pub fn note_sync_error(&self, error: &SyncError) {
        if *error != SyncError::Unauthorized {
            return;
        }
        let signed_out = self.shared.state.send_if_modified(|state| {
            let was_approved = *state == LoginState::Approved;
            if was_approved {
                *state = LoginState::SignedOut;
            }
            was_approved
        });
        if signed_out {
            tracing::warn!("the server rejected the device token; signed out");
        }
    }

    /// Runs `work` while the session is signed in: `None` when it is not signed in at the start or signs out
    /// meanwhile, in which case `work` is dropped at its next await. Pairs and the index are not touched.
    pub async fn while_signed_in<F: Future>(&self, work: F) -> Option<F::Output> {
        let mut state = self.shared.state.subscribe();
        tokio::select! {
            biased;
            _ = state.wait_for(|state| *state != LoginState::Approved) => None,
            output = work => Some(output),
        }
    }

    fn open_browser(&self, url: &str) {
        if ServerUrl::parse(url, allow_plain_http()).is_err() {
            tracing::warn!("the verification URL is not an https URL; the browser was not opened");
            return;
        }
        if let Err(error) = self.browser.open(url) {
            tracing::warn!(%error, "the browser could not be opened; the URL is shown to the user");
        }
    }

    fn abort_attempt(&self) {
        if let Some(task) = self.attempt_slot().take() {
            task.abort();
        }
    }

    fn attempt_slot(&self) -> std::sync::MutexGuard<'_, Option<JoinHandle<()>>> {
        self.attempt.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.abort_attempt();
    }
}

/// Runs a bounded token store call off the async runtime.
async fn blocking<T, F>(call: F) -> Result<T, LoginError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, TokenStoreError> + Send + 'static,
{
    match tokio::task::spawn_blocking(call).await {
        Ok(result) => result.map_err(LoginError::Store),
        Err(error) => {
            tracing::error!(%error, "a token store task failed");
            Err(LoginError::Internal)
        }
    }
}
