//! The typed `sync` section (13 §5). Keys are rows `sync_*` of the shared key table, so loading, ranges,
//! the `SUNNYCLOUD_SYNC_*` names and client reload are `sc_config`'s; this type is the view the core reads.

use std::time::Duration;

use sc_config::{ClientSettings, ConfigError, KeyProblem, Reason};
use sync_core_proto::RetryPolicy;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncConfig {
    pub server_url: String,
    /// `desktop`, `android` or `ios`.
    pub client_family: String,
    pub client_version: String,
    pub retry_base: Duration,
    pub retry_max: Duration,
    pub max_attempts_per_block: u32,
    pub connect_timeout: Duration,
    pub request_idle_timeout: Duration,
    /// Before the clamp to `ServerInfo.longpoll_max_wait`, which the connect step applies.
    pub longpoll_wait: Duration,
    pub longpoll_client_margin: Duration,
    pub h2_stream_window: u64,
    pub h2_conn_window: u64,
    pub keepalive_interval: Duration,
    /// Percent of one core the hashing may use while active (25 = 0.25 of a core).
    pub hash_cpu_share: u8,
    pub hash_checkpoint_blocks: u32,
    pub batch_max_bytes_in_flight: u64,
}

impl SyncConfig {
    /// Reads the `sync_*` keys of an already validated section. Missing build-injected keys are reported together.
    pub fn from_settings(settings: &ClientSettings) -> Result<Self, ConfigError> {
        let missing: Vec<KeyProblem> = [
            ("sync_server_url", settings.sync_server_url.is_none()),
            ("sync_client_family", settings.sync_client_family.is_none()),
            (
                "sync_client_version",
                settings.sync_client_version.is_none(),
            ),
        ]
        .into_iter()
        .filter(|(_, absent)| *absent)
        .map(|(key, _)| KeyProblem::new(key, Reason::MissingRequired))
        .collect();
        let (Some(server_url), Some(client_family), Some(client_version)) = (
            settings.sync_server_url.clone(),
            settings.sync_client_family.clone(),
            settings.sync_client_version.clone(),
        ) else {
            return Err(ConfigError::Invalid(missing));
        };
        Ok(Self {
            server_url,
            client_family,
            client_version,
            retry_base: settings.sync_retry_base.get(),
            retry_max: settings.sync_retry_max.get(),
            max_attempts_per_block: clamp_u32(settings.sync_max_attempts_per_block),
            connect_timeout: settings.sync_connect_timeout.get(),
            request_idle_timeout: settings.sync_request_idle_timeout.get(),
            longpoll_wait: settings.sync_longpoll_wait.get(),
            longpoll_client_margin: settings.sync_longpoll_client_margin.get(),
            h2_stream_window: settings.sync_h2_stream_window.bytes(),
            h2_conn_window: settings.sync_h2_conn_window.bytes(),
            keepalive_interval: settings.sync_keepalive_interval.get(),
            hash_cpu_share: u8::try_from(settings.sync_hash_cpu_share).unwrap_or(100),
            hash_checkpoint_blocks: clamp_u32(settings.sync_hash_checkpoint_blocks),
            batch_max_bytes_in_flight: settings.sync_batch_max_bytes_in_flight.bytes(),
        })
    }

    /// Backoff limits shared by the request path and the upload planner.
    pub fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy {
            base: self.retry_base,
            max: self.retry_max,
            max_attempts_per_block: self.max_attempts_per_block,
        }
    }
}

/// The table ranges keep counts far below `u32::MAX`; the clamp only makes the conversion total.
fn clamp_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
