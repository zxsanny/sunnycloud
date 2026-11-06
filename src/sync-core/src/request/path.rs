use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::Bytes;
use sc_clock::{CancellationToken, Clock};
use sc_error::CorrelationId;
use sync_core_proto::RetryPolicy;

use super::jitter::jitter;
use super::transport::{Method, Transport, WireRequest, WireResponse};
use crate::error::{Mapped, SyncError};
use crate::host::CredentialProvider;
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

/// The one way a request leaves the client. It reads the credential per request, ends with `Cancelled`
/// after [`cancel`](Self::cancel), stops all work after `account_not_active`, waits out `Retry-After`,
/// and turns every non-2xx answer into a value through the wire table.
pub struct RequestPath {
    transport: Arc<dyn Transport>,
    credentials: Arc<dyn CredentialProvider>,
    clock: Arc<dyn Clock>,
    policy: RetryPolicy,
    cancel: CancellationToken,
    account_stopped: AtomicBool,
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
        if self.cancel.is_cancelled() {
            Err(SyncError::Cancelled)
        } else if self.account_stopped.load(Ordering::Acquire) {
            Err(SyncError::AccountNotActive)
        } else {
            Ok(())
        }
    }

    /// Sends `request`, retrying transport failures and 429 up to `max_attempts` sends. `op` names the
    /// operation in logs.
    pub async fn send(&self, op: &'static str, request: Request) -> Result<WireResponse, Mapped> {
        let max_attempts = request.max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.check().map_err(Mapped::Error)?;
            let failure = match self.send_once(&request).await {
                Ok(response) => return Ok(response),
                Err(failure) => failure,
            };
            if let Mapped::Error(error) = &failure.mapped {
                self.note_failure(op, attempt, &failure, error);
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

    async fn send_once(&self, request: &Request) -> Result<WireResponse, Failure> {
        let credential = self.credentials.bearer();
        let wire = WireRequest {
            method: request.method,
            target: request.target.clone(),
            headers: request.headers.clone(),
            bearer: Some(credential.access),
            body: request.body.clone(),
        };
        let sent = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(Failure::error(SyncError::Cancelled)),
            sent = self.transport.send(wire) => sent,
        };
        let response = sent.map_err(|kind| Failure::error(SyncError::Transport(kind)))?;
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
