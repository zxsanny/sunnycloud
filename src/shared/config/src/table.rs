//! The closed key table of `common-helpers/01_helper_config.md`, one row per key.
//!
//! A row reads `key: FieldType = spec;`. The macro turns the rows into the [`Settings`] struct,
//! the [`TABLE`] and the binding code, so a key has exactly one spelling.
//!
//! Field types: `Text` (string), `Flag` (`true`/`on`, `false`/`off`), `Count`, `Size` (`4MiB`),
//! `Dur` (`5m`), `ListenAddr`, `List` (comma separated), `Environment`, `Sec` (secret text).
//! `Opt<T>` marks a key that may be absent; a key whose row names no default and is not `Opt`
//! is required. Specs: `dflt("<default text>", readers)` or `unset(readers)`, then optionally
//! `.range(min, max)`, `.one_of(&[..])`, `.fixed()`.

use crate::bind::Binder;
use crate::spec::readers::{A, ALL, AW, C, R, W, WR};
use crate::spec::{KeySpec, Readers};
use crate::value::{DurationSetting, Environment, ListenAddr, Secret, Size};

type Text = String;
type Flag = bool;
type Count = u64;
type Dur = DurationSetting;
type List = Vec<String>;
type Sec = Secret<String>;
type Opt<T> = Option<T>;

const fn dflt(default: &'static str, read_by: Readers) -> KeySpec {
    KeySpec::with_default(default, read_by)
}

const fn unset(read_by: Readers) -> KeySpec {
    KeySpec::without_default(read_by)
}

const LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

macro_rules! settings_table {
    ($( $key:ident : $ty:ty = $spec:expr; )*) => {
        /// Every key of the table as a typed field. A binary binds and validates the fields whose
        /// row lists it; the other fields hold their default, or a neutral value without one.
        #[derive(Debug)]
        pub struct Settings {
            $( pub $key: $ty, )*
        }

        /// The key table, in documentation order.
        pub static TABLE: &[KeySpec] = &[
            $( $spec.named::<$ty>(stringify!($key)), )*
        ];

        impl Settings {
            pub(crate) fn bind(binder: &mut Binder<'_>) -> Self {
                Settings {
                    $( $key: binder.bind::<$ty>(&$spec.named::<$ty>(stringify!($key))), )*
                }
            }
        }
    };
}

settings_table! {
    // General (11, 12). `environment` is read by every binary: they all need it for the floors.
    environment: Environment = dflt("production", ALL);
    public_url: Text = unset(A);
    trusted_proxies: List = dflt("", A);
    internal_addr: ListenAddr = dflt("0.0.0.0:8081", A);
    ops_addr: ListenAddr = dflt("0.0.0.0:9090", A);
    // `log` and `log_level` are also read by the worker (AZ-3868): its logging setup takes the same two keys.
    log: Text = dflt("text", AW).one_of(&["text", "json"]);
    log_level: Text = dflt("info", AW).one_of(LOG_LEVELS);
    shutdown_grace: Dur = dflt("25s", A);
    min_client_version: Opt<Text> = unset(A);

    // HTTP (11)
    max_connections: Count = dflt("4096", A);
    h1_header_timeout: Dur = dflt("10s", A);
    h2_stream_window: Size = dflt("4MiB", A);
    h2_conn_window: Size = dflt("32MiB", A);
    h2_max_streams: Count = dflt("100", A);
    h2_max_pending_reset: Count = dflt("20", A);
    h2_max_header_list: Size = dflt("16KiB", A);
    h2_keepalive_interval: Dur = dflt("30s", A);
    h2_keepalive_timeout: Dur = dflt("20s", A);
    request_timeout: Dur = dflt("60s", A);
    body_idle_timeout: Dur = dflt("30s", A);

    // Database (07)
    db_api_url: Sec = unset(A);
    db_migrator_url: Opt<Sec> = unset(A);
    db_pool_max: Count = dflt("50", A);
    db_acquire_timeout: Dur = dflt("5s", A);
    db_idle_in_tx_timeout: Dur = dflt("15s", A);
    db_statement_timeout: Dur = dflt("30s", A);
    db_recovery_budget: Dur = dflt("60s", A);

    // Identity (01)
    access_token_ttl: Dur = dflt("15m", A);
    refresh_token_ttl: Dur = dflt("30d", A);
    refresh_reuse_grace: Dur = dflt("30s", A).range("1s", "60s");
    session_cache_ttl: Dur = dflt("2s", A);
    session_last_seen_interval: Dur = dflt("1m", A);
    device_code_ttl: Dur = dflt("10m", A).range("0s", "10m");
    device_poll_interval: Dur = dflt("5s", A);
    oauth_state_ttl: Dur = dflt("10m", A);
    login_code_ttl: Dur = dflt("2m", A);
    invite_ttl: Dur = dflt("7d", A);
    setup_token_ttl: Dur = dflt("7d", A);
    provider_timeout: Dur = dflt("10s", A).range("1s", "30s");
    allowlist_emails: Opt<List> = unset(A);
    github_client_id: Opt<Text> = unset(A);
    oidc_issuer: Opt<Text> = unset(A);
    oidc_client_id: Opt<Text> = unset(A);
    geoip_db_path: Opt<Text> = unset(A);
    allow_plain_http_dev: Flag = dflt("off", A);
    provider_github_client_secret: Opt<Sec> = unset(A);
    provider_oidc_client_secret: Opt<Sec> = unset(A);
    setup_token: Opt<Sec> = unset(A);

    // Abuse limits (helper 08)
    device_grant_ip_limit: Opt<Count> = unset(A);
    invalid_token_ip_limit: Opt<Count> = unset(A);
    ratelimit_max_entries: Count = dflt("100000", A);

    // Tree and feed (02, 05)
    tombstone_horizon: Dur = dflt("90d", A).range("90d", "365d");
    listing_default_limit: Count = dflt("1000", A);
    listing_max_limit: Opt<Count> = unset(A);
    cascade_batch_size: Count = dflt("5000", A);
    purge_batch_size: Opt<Count> = unset(A);
    tree_maintenance_interval: Opt<Dur> = unset(A);
    conflict_copy_time_zone: Text = dflt("UTC", A);
    longpoll_wait_max: Dur = dflt("30s", A).range("1s", "55s");
    longpoll_auth_recheck: Dur = dflt("2s", A);
    feed_safety_recheck: Dur = dflt("10s", A);
    feed_page_limit_default: Count = dflt("500", A);
    feed_page_limit_max: Opt<Count> = unset(A);
    max_holds_per_principal: Count = dflt("4", A);

    // Transfer (03)
    upload_session_ttl: Dur = dflt("24h", A).range("1m", "7d");
    max_declared_size: Size = dflt("50GiB", A);
    max_open_sessions: Count = dflt("8", A);
    max_staged: Count = dflt("1000", A);
    checkpoint_bytes: Size = dflt("8MiB", A);
    checkpoint_interval: Dur = dflt("1s", A);
    max_lanes: Count = dflt("4", A);
    batch_max_entries: Count = dflt("500", A);
    commit_receipt_ttl: Dur = dflt("48h", A);
    max_buffered_block_bytes: Size = dflt("1GiB", A);
    session_reap_interval: Dur = dflt("1m", A);
    receipt_janitor_interval: Dur = dflt("1h", A);

    // Sharing (06)
    share_default_ttl: Dur = dflt("7d", A).range("1m", "365d");
    share_max_ttl: Dur = dflt("365d", A).range("1m", "3650d");
    share_lookup_cache: Dur = dflt("1s", A);
    share_grant_ttl: Dur = dflt("10m", A);
    share_wrong_password_limit: Count = dflt("10", A);
    share_wrong_password_window: Dur = dflt("10m", A);
    share_invalid_token_limit: Count = dflt("100", A);
    share_password_max_concurrency: Opt<Count> = unset(A);
    share_argon2_params: Opt<Text> = unset(A);
    share_grant_key: Sec = unset(A);

    // Admin and quota (07, 07a)
    account_delete_batch_size: Count = dflt("5000", A);
    account_delete_interval: Opt<Dur> = unset(A);
    account_delete_alert_after: Dur = dflt("10m", A);
    account_delete_deadline: Dur = dflt("15m", A).range("1m", "60m");
    health_check_timeout: Opt<Dur> = unset(A);
    admin_list_page_size: Opt<Count> = unset(A);
    default_quota: Size = dflt("100GiB", A);
    audit_retention: Dur = dflt("365d", A);

    // Blobstore (04)
    blob_root: Opt<Text> = unset(A);
    retention: Dur = dflt("7d", A).range("0s", "90d");
    orphan_sweep_age: Dur = dflt("48h", A).range("1m", "30d");
    orphan_mode: Text = dflt("delete", A);
    orphan_sweep_interval: Dur = dflt("24h", A);
    blob_free_reserve: Size = dflt("20GiB", A);
    gc_interval: Dur = dflt("5m", A);
    gc_batch: Count = dflt("100", A);
    gc_claim_lease: Dur = dflt("10m", A);
    gc_cycle_budget: Dur = dflt("30s", A);
    scrub_interval: Dur = dflt("7d", A);
    scrub_cycle_budget: Dur = dflt("30m", A);
    scrub_max_rate: Size = dflt("50MiB", A);
    scrub_pause_active_streams: Count = dflt("8", A);
    blob_sync_coalesce_delay: Dur = dflt("0ms", A);
    blob_preallocate: Flag = dflt("true", A);
    blob_read_ahead_chunks: Count = dflt("16", A);
    blob_io_threads: Opt<Count> = unset(A);
    master_key_file: Sec = unset(A);
    master_key_ring: Opt<Text> = unset(A);
    master_key_current: Opt<Count> = unset(A);

    // Media jobs (08)
    job_lease: Dur = dflt("5m", A).range("5s", "1h");
    job_attempts: Count = dflt("3", A).range("1", "10");
    job_retry_backoff: Dur = dflt("30s", A);
    bulk_queue_threshold: Count = dflt("200", A);
    job_terminal_ttl: Dur = dflt("14d", A);
    job_reap_interval: Dur = dflt("1m", A);
    derived_janitor_interval: Dur = dflt("1h", A);
    derived_max_thumb: Size = dflt("5MiB", A);
    derived_max_preview: Size = dflt("20MiB", A);
    derived_max_transcode: Size = dflt("10GiB", A);
    worker_api_token: Sec = unset(AW);
    worker_api_token_next: Opt<Sec> = unset(A);

    // Worker (09)
    worker_id: Opt<Text> = unset(W);
    worker_api_url: Text = dflt("http://api:8081", W);
    worker_sock_interactive: Opt<Text> = unset(W);
    worker_sock_general: Opt<Text> = unset(W);
    worker_scratch_interactive: Opt<Text> = unset(W);
    worker_scratch_general: Opt<Text> = unset(W);
    worker_claim_poll_interval: Dur = dflt("500ms", W);
    worker_status_interval: Dur = dflt("30s", W);
    decoder_silence_timeout: Dur = dflt("20s", W);
    decoder_cancel_grace: Dur = dflt("5s", W);
    worker_max_input_photo: Size = dflt("256MiB", W);
    worker_max_input_video: Size = dflt("10GiB", W);
    worker_scratch_general_cap: Size = dflt("24GiB", W);
    worker_shutdown_grace: Dur = dflt("30s", W);
    worker_ops_listen: Text = dflt(":9090", W);

    // Limits (09 sends, 10 clamps)
    photo_cpu: Count = dflt("2", WR);
    photo_memory: Size = dflt("2GiB", WR);
    photo_job_timeout: Dur = dflt("120s", WR);
    video_cpu: Count = dflt("4", WR);
    video_memory: Size = dflt("4GiB", WR);
    video_job_timeout: Dur = dflt("40m", WR);
    decoder_stall_kill: Dur = dflt("90s", WR);
    nvenc: Text = dflt("off", WR);

    // Decoder (10)
    decoder_socket_path: Text = dflt("/run/decoder/decoder.sock", R);
    decoder_gid: Opt<Count> = unset(WR);
    decoder_scratch_root: Text = dflt("/scratch", R);
    decoder_alive_interval: Dur = dflt("5s", R);
    decoder_watchdog: Dur = dflt("20s", R);
    decoder_memory_poll_interval: Dur = dflt("1s", R);
    photo_max_pixels: Count = dflt("250000000", R);
    decoder_max_output_photo: Size = dflt("32MiB", WR);
    decoder_max_output_video: Size = dflt("10GiB", WR);
    decoder_format_whitelist: Opt<List> = unset(R);
    decoder_codec_whitelist: Opt<List> = unset(R);

    // Vault (20, v1.2)
    max_vault_file_bytes: Size = dflt("100MiB", A);
    max_vault_header_bytes: Opt<Size> = unset(A);

    // CLI (12)
    cli_confirm_required: Flag = dflt("on", A);
    cli_progress_interval: Dur = dflt("2s", A);

    // Sync client (13, section `sync`). The first three are set by the host build or settings and are
    // required by `SyncConfig`, not by the loader, so a client that never syncs starts without them.
    // `sync_hash_cpu_share` is a percentage of one core: the table has no fractional type.
    sync_server_url: Opt<Text> = unset(C);
    sync_client_family: Opt<Text> = unset(C).one_of(&["desktop", "android", "ios"]);
    sync_client_version: Opt<Text> = unset(C);
    sync_retry_base: Dur = dflt("500ms", C).range("10ms", "1m");
    sync_retry_max: Dur = dflt("30s", C).range("1s", "10m");
    sync_max_attempts_per_block: Count = dflt("8", C).range("1", "100");
    sync_connect_timeout: Dur = dflt("10s", C).range("1s", "2m");
    sync_request_idle_timeout: Dur = dflt("60s", C).range("5s", "10m");
    sync_longpoll_wait: Dur = dflt("30s", C).range("1s", "55s");
    sync_longpoll_client_margin: Dur = dflt("10s", C).range("1s", "1m");
    sync_h2_stream_window: Size = dflt("8MiB", C).range("64KiB", "256MiB");
    sync_h2_conn_window: Size = dflt("32MiB", C).range("64KiB", "1GiB");
    sync_keepalive_interval: Dur = dflt("20s", C).range("5s", "5m");
    sync_hash_cpu_share: Count = dflt("25", C).range("1", "100");
    sync_hash_checkpoint_blocks: Count = dflt("32", C).range("1", "1024");
    sync_batch_max_bytes_in_flight: Size = dflt("64MiB", C).range("2MiB", "1GiB");

    // Client test overrides (14)
    desktop_settle: Dur = dflt("2s", C).range("200ms", "30s");
    desktop_rescan_interval: Dur = dflt("15m", C).range("10s", "24h");
    // Release value of the delete guard (14): a batch of this many deletions in one direction holds the pair.
    desktop_mass_delete_threshold: Count = dflt("200", C).range("1", "1000000");

    // Release-fixed client values (helper 01 Rules). Row names are not in the helper table.
    updater_source: Opt<Text> = unset(C).fixed();
    updater_public_key_1: Opt<Text> = unset(C).fixed();
    updater_public_key_2: Opt<Text> = unset(C).fixed();
    min_api_family: Opt<Text> = unset(C).fixed();
    https_required: Opt<Flag> = unset(C).fixed();
}
