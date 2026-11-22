//! The `HttpConfig` of 11 from `serve`'s settings: a field-for-field mapping, no rule. 11's `HttpServer::new`
//! checks what it cannot use.

use sc_config::Settings;
use sunnycloud_http_api::{CredentialFormat, HttpConfig, ServerInfo, WorkerOutputLimits};

/// What `server-info` advertises when no minimum client version is configured: every version is at least this.
const NO_MINIMUM: &str = "0.0.0";

pub(super) fn http_config(settings: &Settings) -> HttpConfig {
    let min_client_version = settings.min_client_version.clone();
    HttpConfig {
        public_url: settings.public_url.clone(),
        trusted_proxies: settings.trusted_proxies.clone(),
        public_addr: settings.public_addr.get(),
        internal_addr: settings.internal_addr.get(),
        ops_addr: settings.ops_addr.get(),
        #[cfg(feature = "test-clock")]
        test_clock_addr: settings.test_clock_addr.get(),
        max_connections: usize::try_from(settings.max_connections).unwrap_or(usize::MAX),
        h1_header_timeout: settings.h1_header_timeout.get(),
        h2_stream_window: saturated(settings.h2_stream_window.bytes()),
        h2_conn_window: saturated(settings.h2_conn_window.bytes()),
        h2_max_streams: saturated(settings.h2_max_streams),
        h2_max_pending_reset: usize::try_from(settings.h2_max_pending_reset).unwrap_or(usize::MAX),
        h2_max_header_list: saturated(settings.h2_max_header_list.bytes()),
        h2_keepalive_interval: settings.h2_keepalive_interval.get(),
        h2_keepalive_timeout: settings.h2_keepalive_timeout.get(),
        request_timeout: settings.request_timeout.get(),
        body_idle_timeout: settings.body_idle_timeout.get(),
        download_write_idle: settings.download_write_idle.get(),
        worker_output_limits: WorkerOutputLimits {
            thumb: settings.derived_max_thumb.bytes(),
            preview: settings.derived_max_preview.bytes(),
            transcode: settings.derived_max_transcode.bytes(),
        },
        shutdown_grace: settings.shutdown_grace.get(),
        server_info: ServerInfo {
            max_lanes: settings.max_lanes,
            batch_max_entries: settings.batch_max_entries,
            max_staged_per_account: settings.max_staged,
            longpoll_max_wait: settings.longpoll_wait_max.get(),
            min_client_version: min_client_version
                .as_deref()
                .map_or(NO_MINIMUM, str::trim)
                .to_owned(),
            share_default_ttl: settings.share_default_ttl.get(),
            share_max_ttl: settings.share_max_ttl.get(),
        },
        min_client_version,
        // No key and no documented source (`server_info.md` open point 2): with `min_client_version` set,
        // `HttpServer::new` refuses the start.
        update_url: None,
        credentials: CredentialFormat {
            session_cookie: settings.session_cookie.clone(),
            binding_cookie: settings.binding_cookie.clone(),
            refresh_cookie_max_age: settings.refresh_token_ttl.get(),
        },
    }
}

/// A value above `u32::MAX` stays above every limit 11 accepts, so 11 refuses it instead of a wrapped value.
fn saturated(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
