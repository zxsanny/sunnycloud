# Helper: Config (typed binding and validation)

**Crate**: `sc_config` (workspace library). **Used by**: every server component (01-12), the worker supervisor and the decoder runner, 13_sync_core, 14_desktop_daemon, 17_android_app (Rust crate `mobile_backup`). Not used by 16_web_app (build-time constants only).

## Purpose
One place that turns files, environment and secrets into typed, validated settings. No raw key lookups in business code; durations are config values with production defaults and short test overrides. One failure strategy: the process refuses to start (or, for clients, refuses to apply the change) with every error listed at once.

## Interface

| Operation | Input | Output | Errors |
|-----------|-------|--------|--------|
| `load::<T: ConfigSection>` | `ConfigSources { env, files, secrets_dir, environment }` | `T` (immutable, `Arc`-shareable) | `ConfigError::Invalid(Vec<KeyProblem>)`, `ConfigError::MissingSecret(key)`, `ConfigError::Unreadable(path, io_kind)` |
| `ConfigSection::validate` | the bound struct | `Vec<KeyProblem>` | none (collects) |
| `DurationSetting::get` | none | `Duration` | none (already validated) |
| `Secret<T>` | value | opaque wrapper; `Debug`/`Display` print `***` | none |
| `reload_client_settings` (clients only) | new settings JSON | applied `T` or `ConfigError::Invalid` | validation errors; the previous settings stay active |

`KeyProblem { key, reason: OutOfRange{min,max} | WrongType | Unknown | MissingRequired | ForbiddenInRelease }`.

## Rules
- **Binding order**: compiled defaults, config file, environment, then secret files (`<KEY>_FILE`, Docker secrets). Later wins.
- **Names**: the environment variable is `SUNNYCLOUD_` plus the key upper-cased (`share_max_ttl` is `SUNNYCLOUD_SHARE_MAX_TTL`). Keys are flat snake case (no dots). Component specs use the key names of the table below; there is no second spelling and no alias.
- **Secrets** are the keys marked S in the table. A secret is read from `SUNNYCLOUD_<KEY>_FILE` (a path); if neither the variable nor the `_FILE` variable is set, the default path is `/run/secrets/<key>`. Secrets never appear in logs, errors or `Debug` output; a missing required secret is `MissingSecret`.
- **Unknown keys**: the table is the closed list for every binary (`sunnycloud`, `sunnycloud-worker`, `decoder-runner`). Any `SUNNYCLOUD_*` variable that is not in the table refuses start, with every offender listed. A binary ignores keys it does not read, so one `.env` serves all containers. Compose-only variables (`PUBLIC_HOST`, `BLOB_PATH`, `WORKER_CPUS`, `WORKER_MEM`, `SUNNYCLOUD_VERSION`) are not read by any binary; `SUNNYCLOUD_VERSION` is the one prefixed name that is tolerated. A component that adds a key adds its row here in the same change.
- **Durations** are `DurationSetting` values written as `2s`, `5m`, `7d`. Every duration key has a production default, a floor and a ceiling. The floors apply only when `environment = production`; `development` and `test` accept shorter values (this is how the test image uses seconds instead of days). A production start with a below-floor value fails with `ForbiddenInRelease`.
- **Release-fixed values** (not overridable by env or settings file in release builds): updater source, the two pinned Ed25519 public keys that verify the signed update manifest (AC-49, S4-3), minimum API version family, HTTPS-required flag for mobile (AC-54 default).
- **Startup validation** also runs cross-field checks owned by the sections (for example `retention` within 0 to 90 days, at least one sign-in provider (FT-N-25), `public_url` is `https` outside development).

## Key-to-env table (authoritative; S = secret file, D = duration)

Env name is `SUNNYCLOUD_` plus the key upper-cased. "Read by" lists the binaries: A = `sunnycloud` (api and CLI), W = `sunnycloud-worker`, R = `decoder-runner`. Defaults of the owner component are repeated only where the deployment documents use the key.

| Group | Keys (default) | Read by |
|---|---|---|
| General (11, 12) | `environment` (`production`), `public_url` (required), `trusted_proxies` (empty), `internal_addr` (`0.0.0.0:8081`), `ops_addr` (`0.0.0.0:9090`; bound to the backend network address in Compose), `log` (`text`; `json` for services; also read by the worker), `log_level` (`info`; also read by the worker), `shutdown_grace` D (25s), `min_client_version` (set by the release; semver, or `family=semver,...`) | A |
| HTTP (11) | `max_connections` (4096), `h1_header_timeout` D (10s), `h2_stream_window` (4MiB), `h2_conn_window` (32MiB), `h2_max_streams` (100), `h2_max_pending_reset` (20), `h2_max_header_list` (16KiB), `h2_keepalive_interval` D (30s), `h2_keepalive_timeout` D (20s), `request_timeout` D (60s), `body_idle_timeout` D (30s) | A |
| Database (07) | `db_api_url` S, `db_migrator_url` S (only the one-shot `migrate` step), `db_pool_max` (50), `db_acquire_timeout` D (5s), `db_idle_in_tx_timeout` D (15s), `db_statement_timeout` D (30s), `db_recovery_budget` D (60s) | A |
| Identity (01) | `access_token_ttl` D (15m), `refresh_token_ttl` D (30d), `refresh_reuse_grace` D (30s), `session_cache_ttl` D (2s), `session_last_seen_interval` D (1m), `device_code_ttl` D (10m, max 10m), `device_poll_interval` D (5s), `oauth_state_ttl` D (10m), `login_code_ttl` D (2m), `invite_ttl` D (7d), `setup_token_ttl` D (7d), `provider_timeout` D (10s), `allowlist_emails`, `github_client_id`, `oidc_issuer`, `oidc_client_id`, `geoip_db_path` (MaxMind mmdb, GeoLite2-City compatible; a missing or invalid file means unknown locations and the service still starts), `allow_plain_http_dev` (off), `provider_github_client_secret` S, `provider_oidc_client_secret` S, `setup_token` S (optional) | A |
| Abuse limits (helper 08) | `device_grant_ip_limit`, `invalid_token_ip_limit`, `ratelimit_max_entries` (100000); AC-20 values are the defaults | A |
| Tree and feed (02, 05) | `tombstone_horizon` D (90d; floor 90d in release, ceiling 365d), `listing_default_limit` (1000), `listing_max_limit`, `cascade_batch_size` (5000), `purge_batch_size`, `tree_maintenance_interval` D, `conflict_copy_time_zone` (`UTC`), `longpoll_wait_max` D (30s; 1s to 55s), `longpoll_auth_recheck` D (2s), `feed_safety_recheck` D (10s), `feed_page_limit_default` (500), `feed_page_limit_max`, `max_holds_per_principal` (4) | A |
| Transfer (03) | `upload_session_ttl` D (24h), `max_declared_size` (50GiB), `max_open_sessions` (8), `max_staged` (1000), `checkpoint_bytes` (8MiB), `checkpoint_interval` D (1s), `max_lanes` (4), `batch_max_entries` (500), `commit_receipt_ttl` D (48h), `max_buffered_block_bytes` (1GiB), `session_reap_interval` D (1m), `receipt_janitor_interval` D (1h) | A |
| Sharing (06) | `share_default_ttl` D (7d), `share_max_ttl` D (365d), `share_lookup_cache` D (1s), `share_grant_ttl` D (10m), `share_wrong_password_limit` (10), `share_wrong_password_window` D (10m), `share_invalid_token_limit` (100/min/IP), `share_password_max_concurrency`, `share_argon2_params` (`m,t,p`; 06 confirms), `share_grant_key` S | A |
| Admin and quota (07, 07a) | `account_delete_batch_size` (5000), `account_delete_interval` D, `account_delete_alert_after` D (10m), `account_delete_deadline` D (15m; 1m to 60m), `health_check_timeout` D, `admin_list_page_size`, `default_quota` (`100GiB`), `audit_retention` D (365d; `0` keeps) | A |
| Blobstore (04) | `blob_root`, `retention` D (7d; 0 to 90d), `orphan_sweep_age` D (48h), `orphan_mode` (`delete`), `orphan_sweep_interval` D (24h), `blob_free_reserve` (20GiB), `gc_interval` D (5m), `gc_batch` (100), `gc_claim_lease` D (10m), `gc_cycle_budget` D (30s), `scrub_interval` D (7d), `scrub_cycle_budget` D (30m), `scrub_max_rate` (50MiB/s), `scrub_pause_active_streams` (8), `blob_sync_coalesce_delay` D (0ms), `blob_preallocate` (true), `blob_read_ahead_chunks` (16), `blob_io_threads` (2 x cores), `master_key_file` S (key id 1), `master_key_ring` (optional `id=path,...`), `master_key_current` (default highest id) | A |
| Media jobs (08) | `job_lease` D (5m; 5s to 1h), `job_attempts` (3; 1 to 10), `job_retry_backoff` D (30s base, doubling), `bulk_queue_threshold` (200), `job_terminal_ttl` D (14d), `job_reap_interval` D (1m), `derived_janitor_interval` D (1h), `derived_max_thumb` (5MiB), `derived_max_preview` (20MiB), `derived_max_transcode` (10GiB), `worker_api_token` S, `worker_api_token_next` S (optional, rotation) | A (W reads `worker_api_token`) |
| Worker (09) | `worker_id` (hostname + random), `worker_api_url` (`http://api:8081`), `worker_sock_interactive`, `worker_sock_general`, `worker_scratch_interactive`, `worker_scratch_general`, `worker_claim_poll_interval` D (500ms), `worker_status_interval` D (30s), `decoder_silence_timeout` D (20s), `decoder_cancel_grace` D (5s), `worker_max_input_photo` (256MiB), `worker_max_input_video` (10GiB), `worker_scratch_general_cap` (24GiB), `worker_shutdown_grace` D (30s), `worker_ops_listen` (`:9090`) | W |
| Limits (09 sends, 10 clamps) | `photo_cpu` (2), `photo_memory` (2GiB), `photo_job_timeout` D (120s), `video_cpu` (4), `video_memory` (4GiB), `video_job_timeout` D (40m), `decoder_stall_kill` D (90s), `nvenc` (`off`; v1.1) | W, R |
| Decoder (10) | `decoder_socket_path` (`/run/decoder/decoder.sock`), `decoder_gid`, `decoder_scratch_root` (`/scratch`), `decoder_alive_interval` D (5s), `decoder_watchdog` D (20s), `decoder_memory_poll_interval` D (1s), `photo_max_pixels` (250000000), `decoder_max_output_photo` (32MiB), `decoder_max_output_video` (10GiB), `decoder_format_whitelist`, `decoder_codec_whitelist` | R |
| Decoder keys also read by the worker (09; scratch group and output caps, added with AZ-3870) | `decoder_gid`, `decoder_max_output_photo`, `decoder_max_output_video` | W and R |
| Vault (20, v1.2) | `max_vault_file_bytes` (100MiB), `max_vault_header_bytes` | A |
| CLI (12) | `cli_confirm_required` (on), `cli_progress_interval` D (2s) | A |
| Sync client (13, section `sync`) | `sync_server_url`, `sync_client_family` (`desktop`, `android`, `ios`) and `sync_client_version` (all unset; `SyncConfig` requires them, the loader does not); `sync_retry_base` D (500ms; 10ms to 1m), `sync_retry_max` D (30s; 1s to 10m), `sync_max_attempts_per_block` N (8; 1 to 100), `sync_connect_timeout` D (10s; 1s to 2m), `sync_request_idle_timeout` D (60s; 5s to 10m), `sync_longpoll_wait` D (30s; 1s to 55s), `sync_longpoll_client_margin` D (10s; 1s to 1m), `sync_h2_stream_window` S (8MiB; 64KiB to 256MiB), `sync_h2_conn_window` S (32MiB; 64KiB to 1GiB), `sync_keepalive_interval` D (20s; 5s to 5m), `sync_hash_cpu_share` N, percent of one core (25; 1 to 100), `sync_hash_checkpoint_blocks` N (32; 1 to 1024), `sync_batch_max_bytes_in_flight` S (64MiB; 2MiB to 1GiB) | clients |
| Client test overrides (14; debug builds and the test harness only) | `desktop_settle` D (2s; 200ms to 30s), `desktop_rescan_interval` D (15m; 10s to 24h) | clients |
| Delete guard (14; client) | `desktop_mass_delete_threshold` C (200; 1 to 1000000): deletions in one direction in one batch that hold the pair (added with AZ-3903) | clients |

Floor / ceiling for duration keys that have no range in the table: `upload_session_ttl` 1m / 7d, `orphan_sweep_age` 1m / 30d, `refresh_reuse_grace` 1s / 60s, `share_default_ttl` 1m / 365d, `share_max_ttl` 1m / 3650d, `provider_timeout` 1s / 30s. The floors bind only in production.

`SUNNYCLOUD_URL` in `tests/environment.md` is a harness variable, not server configuration. 

## Test overrides
`environment = test` loads `SUNNYCLOUD_*` values from `.env.test` (short durations). The `-test` images add the clock control (03_helper_clock). Clients take overrides from a settings file only in debug builds.

## Tests
FT-P-17 (retention range rejected), FT-N-25 (no provider, refuse start), NFT-SEC-38 (no secrets in logs or images), NFT-SEC-33 step 3 (updater override ignored), FT-P-49 (setting change applied without restart).

## Risks
- Floors hide a real misconfiguration only in non-production: mitigated by logging the active `environment` at start.
- A settings reload on clients must be atomic; `reload_client_settings` swaps one `Arc`.
