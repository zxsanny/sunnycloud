//! The change feed (`sync_api.md`: `poll_changes`, `head_cursor`). A poll is one request that returns a checked
//! page; `has_more` is the caller's cue to poll again at once, so nothing here waits for it. The client rests
//! only after a failure, and the next poll starts after that rest (`Backoff`).

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use sc_clock::{Clock, MonotonicInstant};
use sc_error::TransportKind;
use serde::Deserialize;
use sync_core_proto::{ChangePage, Cursor, PageRule, RetryPolicy, ServerInfo, check_page};

use crate::config::SyncConfig;
use crate::error::SyncError;
use crate::request::{Method, Request, RequestPath, jitter};
use crate::tree_ops::{decode, send};

const CHANGES: &str = "/api/v1/changes";

/// `GET /api/v1/changes/head` answers `{"cursor": "E.S"}` (the contracts name no shape; open point).
#[derive(Deserialize)]
struct HeadBody {
    cursor: Cursor,
}

/// The failures in a row and the earliest instant the next poll may start. State of one feed loop: a client
/// serves one account, and the contract puts the retry rule in the client (`description.md` § Feed).
#[derive(Default)]
struct Backoff {
    failures: u32,
    not_before: Option<MonotonicInstant>,
}

pub(crate) struct Feed {
    clock: Arc<dyn Clock>,
    /// `ServerInfo.longpoll_max_wait`, already range-checked by `connect`.
    cap: Duration,
    margin: Duration,
    policy: RetryPolicy,
    backoff: Mutex<Backoff>,
}

impl Feed {
    pub(crate) fn new(clock: Arc<dyn Clock>, config: &SyncConfig, info: &ServerInfo) -> Self {
        Self {
            clock,
            cap: Duration::from_secs(info.longpoll_max_wait),
            margin: config.longpoll_client_margin,
            policy: config.retry_policy(),
            backoff: Mutex::new(Backoff::default()),
        }
    }

    /// One page after `cursor`. The server is asked to hold the request for `wait` clamped to its cap (whole
    /// seconds on the wire), and the client gives up at the clamped wait plus `sync.longpoll_client_margin`
    /// with `Transport(Timeout)`. After a failure that is worth retrying the call first rests for the backoff
    /// delay: `retry_base` doubling up to `retry_max`, or `Retry-After`, each with jitter.
    pub(crate) async fn poll_changes(
        &self,
        path: &RequestPath,
        cursor: Cursor,
        wait: Duration,
        limit: Option<u32>,
    ) -> Result<ChangePage, SyncError> {
        self.rest(path).await?;
        let wait = self.clamp(wait);
        let request = Request {
            method: Method::Get,
            target: changes_target(cursor, wait, limit),
            headers: Vec::new(),
            body: Bytes::new(),
            max_attempts: 1,
        };
        let result = self
            .within(wait.saturating_add(self.margin), async {
                let response = send(path, "poll_changes", request).await?;
                checked_page(cursor, decode("poll_changes", response)?)
            })
            .await;
        self.note(&result);
        result
    }

    pub(crate) fn clamp(&self, wait: Duration) -> Duration {
        wait.min(self.cap)
    }

    /// Ends `work` with `Transport(Timeout)` when `limit` passes first.
    async fn within<T>(
        &self,
        limit: Duration,
        work: impl Future<Output = Result<T, SyncError>>,
    ) -> Result<T, SyncError> {
        let deadline = self
            .clock
            .monotonic()
            .checked_add(limit)
            .ok_or(SyncError::Transport(TransportKind::Other))?;
        tokio::select! {
            biased;
            done = work => done,
            slept = self.clock.sleep_until_monotonic(deadline) => match slept {
                Ok(()) => Err(SyncError::Transport(TransportKind::Timeout)),
                Err(_) => Err(SyncError::Cancelled),
            },
        }
    }

    async fn rest(&self, path: &RequestPath) -> Result<(), SyncError> {
        let until = self.state().not_before;
        match until {
            Some(at) => {
                path.pause(at.saturating_duration_since(self.clock.monotonic()))
                    .await
            }
            None => path.check(),
        }
    }

    fn note<T>(&self, result: &Result<T, SyncError>) {
        let mut state = self.state();
        match result {
            Err(SyncError::Cancelled) => {}
            Err(error) if backs_off(error) => {
                state.failures = state.failures.saturating_add(1);
                let delay = retry_delay(
                    &self.policy,
                    state.failures,
                    error,
                    jitter(self.policy.base),
                );
                state.not_before = self.clock.monotonic().checked_add(delay);
            }
            Ok(_) | Err(_) => *state = Backoff::default(),
        }
    }

    /// Every update replaces whole values under the lock, so a panic elsewhere cannot leave a half-written state.
    fn state(&self) -> std::sync::MutexGuard<'_, Backoff> {
        self.backoff.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `GET /api/v1/changes/head`: the account's current cursor, taken before a listing (F14).
pub(crate) async fn head_cursor(path: &RequestPath) -> Result<Cursor, SyncError> {
    let request = Request {
        method: Method::Get,
        target: format!("{CHANGES}/head"),
        headers: Vec::new(),
        body: Bytes::new(),
        max_attempts: path.default_attempts(),
    };
    let head: HeadBody = decode("head_cursor", send(path, "head_cursor", request).await?)?;
    Ok(head.cursor)
}

fn changes_target(cursor: Cursor, wait: Duration, limit: Option<u32>) -> String {
    let mut target = format!("{CHANGES}?cursor={cursor}&wait={}", wait.as_secs());
    if let Some(limit) = limit {
        target.push_str(&format!("&limit={limit}"));
    }
    target
}

/// A page that breaks one of P1..P6 is refused whole; the rule is named, the offending ids are not logged.
fn checked_page(cursor: Cursor, page: ChangePage) -> Result<ChangePage, SyncError> {
    match check_page(cursor, &page) {
        Ok(()) => Ok(page),
        Err(violation) => {
            tracing::warn!(rule = %violation.rule, "feed page refused");
            Err(SyncError::ProtocolViolation {
                detail: rule_detail(violation.rule),
            })
        }
    }
}

fn rule_detail(rule: PageRule) -> &'static str {
    match rule {
        PageRule::P1 => "feed page rule P1: items not ascending by (seq, node_id)",
        PageRule::P2 => "feed page rule P2: item at or before the request cursor",
        PageRule::P3 => "feed page rule P3: item after the next cursor",
        PageRule::P4 => "feed page rule P4: next cursor in another epoch",
        PageRule::P5 => "feed page rule P5: next cursor before the request cursor",
        PageRule::P6 => "feed page rule P6: duplicate (seq, node_id)",
    }
}

/// Failures that may pass by themselves, or that a repeat at once would only make worse.
fn backs_off(error: &SyncError) -> bool {
    matches!(
        error,
        SyncError::Transport(_)
            | SyncError::RateLimited { .. }
            | SyncError::Unexpected { .. }
            | SyncError::ProtocolViolation { .. }
    )
}

/// The rest before the next poll after `failures` failures in a row: `Retry-After` plus `jitter`, or
/// `min(retry_max, retry_base * 2^(failures - 1) + jitter)`.
pub(crate) fn retry_delay(
    policy: &RetryPolicy,
    failures: u32,
    error: &SyncError,
    jitter: Duration,
) -> Duration {
    match error {
        SyncError::RateLimited {
            retry_after: Some(wait),
        } => wait.saturating_add(jitter),
        _ => policy
            .backoff(failures)
            .saturating_add(jitter)
            .min(policy.max),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sc_error::TransportKind;
    use sync_core_proto::{Cursor, RetryPolicy};

    use std::sync::Arc;

    use sc_clock::SystemClock;
    use serde_json::json;
    use sync_core_proto::ServerInfo;

    use super::{Feed, changes_target, retry_delay};
    use crate::config::SyncConfig;
    use crate::error::SyncError;

    fn feed_with_cap(cap_seconds: u64) -> Feed {
        let config = SyncConfig {
            server_url: "https://sync.test".to_owned(),
            client_family: "desktop".to_owned(),
            client_version: "1.0.0".to_owned(),
            retry_base: Duration::from_millis(500),
            retry_max: Duration::from_secs(30),
            max_attempts_per_block: 8,
            connect_timeout: Duration::from_secs(10),
            request_idle_timeout: Duration::from_secs(60),
            longpoll_wait: Duration::from_secs(30),
            longpoll_client_margin: Duration::from_secs(10),
            h2_stream_window: 8 << 20,
            h2_conn_window: 32 << 20,
            keepalive_interval: Duration::from_secs(20),
            hash_cpu_share: 25,
            hash_checkpoint_blocks: 32,
            batch_max_bytes_in_flight: 64 << 20,
        };
        let info: ServerInfo = serde_json::from_value(json!({
            "block_size": 2_097_152,
            "max_lanes": 4,
            "small_file_max": 2_097_152,
            "batch_max_entries": 500,
            "max_staged_per_account": 1000,
            "longpoll_max_wait": cap_seconds,
            "min_client_version": "1.0.0",
            "api_versions": ["v1"],
        }))
        .expect("a valid server-info body");
        Feed::new(Arc::new(SystemClock::new()), &config, &info)
    }

    #[test]
    fn a_wait_of_30_seconds_under_a_cap_of_20_is_asked_as_20_and_a_smaller_wait_is_kept() {
        // Arrange
        let feed = feed_with_cap(20);

        // Act
        let clamped = feed.clamp(Duration::from_secs(30));
        let kept = feed.clamp(Duration::from_secs(5));

        // Assert
        assert_eq!(clamped, Duration::from_secs(20));
        assert_eq!(kept, Duration::from_secs(5));
        assert_eq!(clamped + feed.margin, Duration::from_secs(30));
    }

    fn policy() -> RetryPolicy {
        RetryPolicy {
            base: Duration::from_millis(500),
            max: Duration::from_secs(30),
            max_attempts_per_block: 8,
        }
    }

    #[test]
    fn delays_double_from_the_base_and_stop_at_the_maximum_with_the_jitter_inside_it() {
        // Arrange
        let error = SyncError::Transport(TransportKind::Timeout);
        let jitter = Duration::from_millis(250);

        // Act
        let delays: Vec<Duration> = (1..=9)
            .map(|failures| retry_delay(&policy(), failures, &error, jitter))
            .collect();

        // Assert
        let millis: Vec<u128> = delays.iter().map(Duration::as_millis).collect();
        assert_eq!(
            millis,
            vec![750, 1250, 2250, 4250, 8250, 16250, 30000, 30000, 30000]
        );
    }

    #[test]
    fn retry_after_is_waited_in_full_and_the_jitter_comes_on_top() {
        // Arrange
        let error = SyncError::RateLimited {
            retry_after: Some(Duration::from_secs(3)),
        };

        // Act
        let delay = retry_delay(&policy(), 1, &error, Duration::from_millis(400));

        // Assert
        assert_eq!(delay, Duration::from_millis(3400));
    }

    #[test]
    fn a_rate_limit_without_retry_after_uses_the_exponential_delay() {
        // Arrange
        let error = SyncError::RateLimited { retry_after: None };

        // Act
        let delay = retry_delay(&policy(), 3, &error, Duration::ZERO);

        // Assert
        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn the_request_names_the_cursor_the_whole_seconds_of_the_wait_and_the_limit() {
        // Arrange
        let cursor = Cursor::new(1, 100);

        // Act
        let with_limit = changes_target(cursor, Duration::from_millis(20_900), Some(50));
        let without_limit = changes_target(cursor, Duration::from_secs(20), None);

        // Assert
        assert_eq!(with_limit, "/api/v1/changes?cursor=1.100&wait=20&limit=50");
        assert_eq!(without_limit, "/api/v1/changes?cursor=1.100&wait=20");
    }
}
