use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use sc_clock::{CancellationToken, Clock};
use sc_error::CorrelationId;
use sync_core_proto::RetryPolicy;

use super::jitter::jitter;
use super::transport::{Method, Transport, WireRequest, WireResponse};
use crate::error::{Mapped, SyncError};
use crate::host::{Credential, CredentialProvider, CredentialUnavailable};
use crate::wire_table::map_problem;

/// One request of an operation. `max_attempts` counts sends: 1 means no retry, so a 429 surfaces as
/// `RateLimited{retry_after}` and the caller decides.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    pub max_attempts: u32,
}

/// Whether a request carries the session credential and is answered to a 401 by a token rotation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Auth {
    Session,
    Anonymous,
}

/// The one way a request leaves the client. It reads the credential per request, ends with `Cancelled`
/// after [`cancel`](Self::cancel), stops all work after `account_not_active`, waits out `Retry-After`,
/// rotates a mobile session's tokens once on a 401 and repeats the request (`refresh.rs`), stops all work
/// when the session or device token is revoked, and turns every non-2xx answer into a value through the wire table.
pub struct RequestPath {
    transport: Arc<dyn Transport>,
    pub(super) credentials: Arc<dyn CredentialProvider>,
    clock: Arc<dyn Clock>,
    policy: RetryPolicy,
    pub(super) cancel: CancellationToken,
    account_stopped: AtomicBool,
    /// Set when the session is revoked, the device token is refused or the rotated pair could not be stored.
    pub(super) auth_stopped: AtomicBool,
    /// Written by the one rotation in flight (later 401s wait on it); read while a request takes its credential.
    pub(super) refresh_gate: tokio::sync::RwLock<()>,
    /// Counts completed rotations: a 401 older than the current value is answered by the rotation that finished.
    pub(super) generation: AtomicU64,
}

impl RequestPath {
    pub fn new(
        transport: Arc<dyn Transport>,
        credentials: Arc<dyn CredentialProvider>,
        clock: Arc<dyn Clock>,
        policy: RetryPolicy,
    ) -> Self {
        Self {
            transport,
            credentials,
            clock,
            policy,
            cancel: CancellationToken::new(),
            account_stopped: AtomicBool::new(false),
            auth_stopped: AtomicBool::new(false),
            refresh_gate: tokio::sync::RwLock::new(()),
            generation: AtomicU64::new(0),
        }
    }

    /// Ends every request in flight and every sleep with `Cancelled`; nothing is sent afterwards.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Sends allowed to an operation without a reason of its own to differ: `sync.max_attempts_per_block`, at least 1.
    pub(crate) fn default_attempts(&self) -> u32 {
        self.policy.max_attempts_per_block.max(1)
    }

    /// Whether work may continue; executors call it between blocks, before building the next request.
    pub fn check(&self) -> Result<(), SyncError> {
        self.check_for(Auth::Session)
    }

    fn check_for(&self, auth: Auth) -> Result<(), SyncError> {
        if self.cancel.is_cancelled() {
            Err(SyncError::Cancelled)
        } else if self.account_stopped.load(Ordering::Acquire) {
            Err(SyncError::AccountNotActive)
        } else if auth == Auth::Session && self.auth_stopped.load(Ordering::Acquire) {
            Err(SyncError::Unauthorized)
        } else {
            Ok(())
        }
    }

    pub(super) fn stop_auth(&self) {
        self.auth_stopped.store(true, Ordering::Release);
    }

    /// A new sign-in ended the stop: the host holds fresh tokens now.
    pub(crate) fn resume_auth(&self) {
        self.auth_stopped.store(false, Ordering::Release);
    }

    /// Whether the host's credential is a desktop device token, which has no refresh token.
    pub(crate) fn is_device_credential(&self) -> Result<bool, SyncError> {
        Ok(self.credential()?.refresh.is_none())
    }

    /// The host's credential for one request. Unavailable is not a refusal: nothing is stopped or rotated.
    pub(super) fn credential(&self) -> Result<Credential, SyncError> {
        self.credentials
            .bearer()
            .map_err(|CredentialUnavailable| SyncError::CredentialUnavailable)
    }

    /// Sends `request`, retrying transport failures and 429 up to `max_attempts` sends. `op` names the
    /// operation in logs. A 401 rotates the session's tokens and repeats the request once, outside `max_attempts`.
    pub async fn send(&self, op: &'static str, request: Request) -> Result<WireResponse, Mapped> {
        self.run(op, request, Auth::Session).await
    }

    /// `send` without credential and without token rotation: the sign-in routes and the rotation itself.
    pub(crate) async fn send_anonymous(
        &self,
        op: &'static str,
        request: Request,
    ) -> Result<WireResponse, Mapped> {
        self.run(op, request, Auth::Anonymous).await
    }

    async fn run(
        &self,
        op: &'static str,
        request: Request,
        auth: Auth,
    ) -> Result<WireResponse, Mapped> {
        let max_attempts = request.max_attempts.max(1);
        let mut attempt = 0;
        let mut rotated = false;
        loop {
            attempt += 1;
            self.check_for(auth).map_err(Mapped::Error)?;
            // Read before the credential: a rotation that finishes in between only makes the next step a no-op.
            let generation = self.generation.load(Ordering::Acquire);
            let failure = match self.send_once(&request, auth).await {
                Ok(response) => return Ok(response),
                Err(failure) => failure,
            };
            if let Mapped::Error(error) = &failure.mapped {
                self.note_failure(op, attempt, &failure, error);
                if auth == Auth::Session && *error == SyncError::Unauthorized && !rotated {
                    self.renew(generation).await.map_err(Mapped::Error)?;
                    rotated = true;
                    attempt -= 1;
                    continue;
                }
                if error.is_retryable() && attempt < max_attempts {
                    let delay = self.delay_for(error, attempt);
                    tracing::warn!(
                        op,
                        attempt,
                        status = failure.status,
                        request_id = failure.request_id.as_deref(),
                        delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                        "retrying request"
                    );
                    self.sleep(delay, failure.mapped).await?;
                    continue;
                }
            }
            return Err(failure.mapped);
        }
    }

    async fn send_once(&self, request: &Request, auth: Auth) -> Result<WireResponse, Failure> {
        let sent = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(Failure::error(SyncError::Cancelled)),
            sent = self.transmit(request, auth) => sent,
        };
        let response = sent.map_err(Failure::error)?;
        if (200..300).contains(&response.status) {
            return Ok(response);
        }
        let headers: Vec<(&str, &str)> = response
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        Err(Failure {
            mapped: map_problem(response.status, &headers, &response.body),
            status: Some(response.status),
            request_id: response
                .header("x-request-id")
                .and_then(CorrelationId::parse)
                .map(|id| id.to_string()),
        })
    }

    /// The credential is read only while no rotation is in flight, so no request carries a new access token
    /// before the host stored it. Without a credential nothing is sent.
    async fn transmit(&self, request: &Request, auth: Auth) -> Result<WireResponse, SyncError> {
        let bearer = match auth {
            Auth::Session => {
                let _idle = self.refresh_gate.read().await;
                Some(self.credential()?.access)
            }
            Auth::Anonymous => None,
        };
        self.transport
            .send(WireRequest {
                method: request.method,
                target: request.target.clone(),
                headers: request.headers.clone(),
                bearer,
                body: request.body.clone(),
            })
            .await
            .map_err(SyncError::Transport)
    }

    /// Stops the account's work on `account_not_active`; logs an answer the table does not know.
    fn note_failure(&self, op: &'static str, attempt: u32, failure: &Failure, error: &SyncError) {
        match error {
            SyncError::AccountNotActive => self.account_stopped.store(true, Ordering::Release),
            SyncError::Unexpected { code, request_id } => tracing::error!(
                op,
                attempt,
                status = failure.status,
                request_id = request_id.as_deref().or(failure.request_id.as_deref()),
                code = %code,
                "unexpected server answer"
            ),
            _ => {}
        }
    }

    fn delay_for(&self, error: &SyncError, attempt: u32) -> Duration {
        let wait = match error {
            SyncError::RateLimited {
                retry_after: Some(retry_after),
            } => *retry_after,
            _ => self.policy.backoff(attempt),
        };
        wait.saturating_add(jitter(self.policy.base))
    }

    /// Rests for `delay` on the injected clock; `Cancelled` ends it early. A delay beyond the clock's range
    /// cannot be waited and is reported as `Cancelled`.
    pub(crate) async fn pause(&self, delay: Duration) -> Result<(), SyncError> {
        if delay.is_zero() {
            return self.check();
        }
        self.sleep(delay, Mapped::Error(SyncError::Cancelled))
            .await
            .map_err(|mapped| match mapped {
                Mapped::Error(error) => error,
                Mapped::Planner(_) => SyncError::Cancelled,
            })
    }

    /// Sleeps on the injected clock; `last` is what the caller gets when the delay cannot be waited at all.
    async fn sleep(&self, delay: Duration, last: Mapped) -> Result<(), Mapped> {
        let Some(at) = self.clock.monotonic().checked_add(delay) else {
            return Err(last);
        };
        let slept = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(Mapped::Error(SyncError::Cancelled)),
            slept = self.clock.sleep_until_monotonic(at) => slept,
        };
        slept.map_err(|_| Mapped::Error(SyncError::Cancelled))
    }
}

/// A request that did not end in 2xx: what the wire table (or the transport) made of it, plus log fields.
struct Failure {
    mapped: Mapped,
    status: Option<u16>,
    request_id: Option<String>,
}

impl Failure {
    fn error(error: SyncError) -> Self {
        Self {
            mapped: Mapped::Error(error),
            status: None,
            request_id: None,
        }
    }
}
