# sunnycloud — Observability

**Status**: draft (planning, no code yet)
**Date**: 2026-11-04
**Sources**: `architecture.md` (Observability confirmed decision, §7 audit logging); `b_ops_licence_scope.md` F18; AC-22, AC-24, AC-25, AC-43, AC-63, AC-64, AC-65.

**Scope**: structured JSON logs, a Prometheus endpoint, health endpoints, an operator `doctor` command and an audit log. **No bundled monitoring stack**, no tracing backend, no OpenTelemetry in v1.0. Operators who run Prometheus, Loki or similar point them at the endpoints below.

## Endpoints

| Endpoint | Listener | Auth | Purpose |
|----------|----------|------|---------|
| `GET :9090/healthz` | Ops listener of the `sunnycloud` API process and of the worker, backend network only, never routed by Caddy | none | Liveness: the process runs and its event loop answers. No dependency checks |
| `GET :9090/readyz` | same | none | Readiness: database reachable, migrations current, master-key check passed, blob volume writable and above the free-space floor, scheduler alive (api); API `:8081` reachable and both decoders reachable (worker, which has no database connection). 200 or 503 with a JSON list of failing checks |
| `GET :9090/metrics` | same | none (private network) | Prometheus text format |
| `GET /api/v1/server-info` | Public, through Caddy | none | `ServerInfo` (version, `api_versions`, `min_client_version`, limits) plus providers, for clients and external uptime probes (AC-64). Reveals nothing beyond what the sign-in page shows |
| `/api/v1/worker-jobs/**` | Internal listener `:8081`, backend network only, never routed by Caddy | worker bearer | Supervisor fetches input and posts results; absent from the public listener (404, AC-79) |
| `POST :9091/test/clock` | `-test` API image only | bearer via `e2e-ctl` | Advance the injectable clock; the release image returns 404, CI-asserted (AC-30) |
| `GET :8081/internal/v1/test/clock-offset` | `-test` API image only, internal listener | worker bearer | Current clock offset, polled by the `-test` supervisor so both processes share one offset; the release image returns 404, CI-asserted |

Compose health checks use `sunnycloud healthcheck` (calls `/readyz`) so no curl is needed in the images. Prometheus reaches `:9090` over the `backend` network, or the operator publishes `127.0.0.1:9090` in a local override. The ops listener is never published on a public interface by default.

## Logging

| Aspect | Rule |
|--------|------|
| Sink | stdout and stderr, one JSON object per line; Docker `json-file` driver with `max-size: 10m`, `max-file: 5` by default; operators may switch the driver (journald, Loki, fluentd) |
| Fields | `ts` (UTC RFC 3339), `level`, `target`, `msg` (constant template), `request_id`, `component` (`api`, `worker`, `decoder-runner`), `account_id` and `node_id` where relevant (opaque IDs), `route`, `status`, `duration_ms`, `err` (error chain) |
| Levels | `error` unexpected failure; `warn` expected-but-notable (rejected upload at the free floor, provider timeout, refresh reuse, mass-delete hold reported by a client); `info` lifecycle (start, migration applied, scrub start/finish, job finished); `debug` off by default and never enabled in release images by a default config |
| Correlation | One `request_id` per request, echoed in the `X-Request-Id` response header and in the error body, so a user report can be matched to the log line |
| Never logged | Secrets, tokens (even hashed), master key material, provider tokens, file names and paths, email addresses, share URLs, setup token (except the single labelled first-start line when the server generated it) |
| Volume | No per-block or per-chunk lines. Per-request lines at `info` only for non-2xx; 2xx are counted in metrics |
| Retention | Operator's concern; the compose default keeps about 50 MB per service |

## Metrics

All names below are written without their prefix; the exported names carry the prefix `sunnycloud_` (for example `sunnycloud_jobs_queued`). Labels are low-cardinality enumerations only: never account, node, file, share or IP values. Histograms use the same bucket set per family so dashboards compose.

### Transfer

| Metric | Type | Labels | Use |
|--------|------|--------|-----|
| `http_requests_total` | counter | `route`, `method`, `status_class` | 5xx rate, 4xx spikes |
| `http_request_duration_seconds` | histogram | `route` | Listing p95 (AC-6), share open (AC-12) |
| `upload_sessions_active` | gauge | | Per-account caps (AC-39) |
| `upload_sessions_total` | counter | `result` (`committed`, `expired`, `aborted`) | |
| `upload_rejections_total` | counter | `reason` (`quota`, `session_cap`, `free_floor`) | AC-39 |
| `blocks_received_total`, `block_bytes_received_total` | counter | | Throughput |
| `block_fsync_duration_seconds` | histogram | | Durability cost (AC-38) |
| `commits_total` | counter | `kind` (`single`, `batch`, `block_list`), `result` | AC-40 |
| `commit_duration_seconds` | histogram | `kind` | Constant-time finish (AC-4) |
| `download_bytes_total` | counter | | |
| `blob_free_bytes`, `blob_free_floor_bytes`, `blob_reserved_bytes` | gauge | | Disk headroom, quota reservations |
| `blob_read_duration_seconds` | histogram | `kind` (`range`, `stream`) | 4 KiB range p95 (AC-43) |

### Sync

| Metric | Type | Labels | Use |
|--------|------|--------|-----|
| `changes_longpoll_active` | gauge | | Concurrent held polls (AC-1) |
| `changes_events_total` | counter | | Feed volume |
| `changes_resync_required_total` | counter | `reason` (`unknown_epoch`, `below_horizon`) | Rises after a restore (AC-41) |
| `sync_conflict_copies_total` | counter | | AC-8 |
| `sync_epoch` | gauge | | Value of the current epoch; changes only on restore |
| `sync_client_versions_total` | counter | `client` (`web`, `android`, `desktop`, `ios`), `supported` (`yes`, `no`) | Spot old clients before raising the minimum (AC-64) |

### Janitors

| Metric | Type | Labels | Use |
|--------|------|--------|-----|
| `janitor_cycles_total` | counter | `task`, `result` (`ok`, `failed`) | Every periodic task run by the scheduler inside `sunnycloud serve` (decision 16); a failed cycle is logged at ERROR and the loop continues |
| `janitor_last_success_timestamp_seconds` | gauge | `task` | A task that stopped succeeding (the scheduler-alive check of `/readyz`) |

### Jobs (workers)

| Metric | Type | Labels | Use |
|--------|------|--------|-----|
| `jobs_queued` | gauge | `kind` (`thumbnail`, `transcode`) | Queue depth |
| `jobs_oldest_age_seconds` | gauge | `kind` | "Is it stuck?" |
| `jobs_total` | counter | `kind`, `result` (`done`, `failed`, `retried`, `timed_out`, `skipped_compliant`) | AC-11 |
| `job_duration_seconds` | histogram | `kind` | |
| `thumbnail_ready_seconds` | histogram | | Upload completion to thumbnail (AC-10) |
| `job_leases_expired_total` | counter | | Crashed or stalled workers |
| `decoder_kills_total` | counter | `reason` (`stall`, `timeout`, `memory`) | AC-45 |
| `derived_bytes_total`, `original_bytes_total` | counter | `kind` | Ratio for AC-44 |
| `worker_selfcheck_ok` | gauge | | 1 when the decoder proved isolation at start (AC-25) |
| `worker_decoder_build_timestamp_seconds` | gauge | | Age of the decoder image (AC-63) |

### Scrub (AC-24)

A run's summary (verified, mismatched and missing counts, duration) is also written as one `scrub_summary` audit event; there is no scrub table (decision 6).

| Metric | Type | Use |
|--------|------|-----|
| `scrub_running` | gauge | |
| `scrub_blobs_verified_total` | counter | Progress |
| `scrub_mismatches_total` | counter | Page on any increase |
| `scrub_missing_total` | counter | Blob referenced but absent |
| `scrub_last_completed_timestamp_seconds` | gauge | Stale scrub alert |
| `scrub_last_duration_seconds` | gauge | |
| `scrub_throttle_sleep_seconds_total` | counter | Proof the throttle works |

### Garbage collection, retention, sweeps

| Metric | Type | Use |
|--------|------|-----|
| `gc_queue_depth` | gauge | Blobs awaiting removal |
| `gc_oldest_due_age_seconds` | gauge | GC running behind |
| `gc_blobs_removed_total`, `gc_bytes_removed_total` | counter | |
| `gc_orphans_swept_total` | counter | 48 h orphan sweep |
| `tombstones_purged_total` | counter | 90-day horizon |
| `upload_sessions_expired_total` | counter | 24 h sessions |
| `account_deletions_total`, `account_deletion_duration_seconds` | counter, histogram | AC-27 (15 min) |

### Authentication and abuse

| Metric | Type | Labels | Use |
|--------|------|--------|-----|
| `auth_signins_total` | counter | `provider`, `result` (`ok`, `denied`, `error`) | |
| `auth_provider_duration_seconds` | histogram | `provider` | |
| `auth_provider_timeouts_total` | counter | `provider` | 10 s timeout hits |
| `auth_rate_limited_total` | counter | `limit` (`signin_ip`, `device_grant_ip`, `device_approval_session`, `device_approval_account`, `device_approval_ip`, `invalid_token_ip`, `share_password`, `share_token_ip`) | AC-20 (helper 08) |
| `auth_refresh_reuse_detected_total` | counter | | Token theft signal (AC-58) |
| `auth_device_grants_total` | counter | `result` | AC-57 |
| `auth_sessions_active` | gauge | | |
| `auth_setup_token_pending` | gauge | | 1 until the first admin exists (AC-23) |
| `share_access_total` | counter | `result` (`ok`, `not_found`, `password_wrong`) | Never per link |

### Process and database

`db_up`, `db_pool_connections{state}`, `db_query_duration_seconds{op}` (grouped, not per statement), `migrations_pending`, `build_info{version,commit,test_clock}`, plus the standard process collector (`process_resident_memory_bytes`, `process_open_fds`, CPU seconds) — the first stalled-connection check of AC-59 reads RSS from it.

## `sunnycloud doctor`

Run as `docker compose exec api sunnycloud doctor` (the CLI is the same `sunnycloud` binary that serves the API). Read-only, no secrets in its output. Exit code 0 (all OK), 1 (warnings), 2 (failures) so scripts and monitoring can use it.

| Check | Fails or warns when |
|-------|---------------------|
| Config | Required value missing, unknown key, malformed duration, out-of-range value |
| Public URL | Not `https`; name does not resolve; TLS handshake to the public URL is not TLS 1.3; not HTTP/2 |
| Proxy | Trusted-proxy list empty (warn: all clients share one address); a 3 MiB probe body to the public URL is rejected (413, the nginx 1 MB default) |
| Database | Unreachable; version is not 18; roles `sunnycloud_migrator` and `sunnycloud` lack or exceed privileges (the API role has DDL, `BYPASSRLS`, or more than INSERT and SELECT on `audit_log`: fail); migrations pending or half applied |
| Master key | Missing, not 32 bytes, does not match the installation's key-check value (fail); only one key backup attested (warn, from `--attest-backup`) |
| Epoch marker | `.epoch` on the blob volume is absent on a volume that holds blobs, or differs from the database epoch: a restore without `admin epoch-bump` (**Fail**, not a warning, decision 5); `serve` also refuses to start in that state |
| Blob volume | Missing, not writable, staging not on the same filesystem; filesystem is neither XFS nor ext4 (warn); free space under the floor |
| Providers | Per enabled provider: discovery or token endpoint reachable within 10 s; prints the **exact redirect URIs** to register in the provider console (AC-22) |
| Setup | Setup token pending (info with age); no admin yet |
| Worker | Self-check result of both decoders and image build date from the last supervisor heartbeat; libheif/libvips/ffmpeg versions; warns when the decoder build is older than 14 days |
| Retention | `RETENTION` is `0` (warn: no safety net); retention shorter than the operator's declared backup interval (`--backup-interval`, warn) |
| Clock | Skew against the database server over 2 s |

`sunnycloud doctor --json` prints the same result for tooling.

## Audit log

| Aspect | Rule |
|--------|------|
| Events | Sign-ins and failures, device approvals and revocations, share create/revoke/access, vault failures (v1.2), admin actions (invites, quota changes, role changes, restore, `rewrap`, `keycheck-init`, `epoch-bump`, `media-requeue`, scrub trigger), scrub summaries and mismatches, account deletion, setup-token use, migrations applied |
| Content | IDs and event type only: actor account ID, target ID, source IP, request ID, result, timestamp. No names, paths, emails or tokens |
| Storage | Append-only table in Postgres: only the audit writer inserts and only the retention purge deletes. `sunnycloud_migrator` owns the table; the API role `sunnycloud` has `INSERT` and `SELECT` only and cannot re-grant itself anything. The retention purge is a `SECURITY DEFINER` function owned by the migrator and run by a janitor; a CI test (role privilege test) fails on any other statement against the table (see `common-helpers/04_helper_audit.md`). Every entry is also written to the JSON log with `"stream":"audit"` so an external collector keeps a copy outside the database |
| Retention | `SUNNYCLOUD_AUDIT_RETENTION`, operator-set (default 365 days, `0` keeps forever) |
| Access | Admin web section (read-only list with filters) and `sunnycloud admin audit export --since <time>` |

## Alert suggestions for operators

Not shipped as a stack. The release bundle includes `examples/prometheus-alerts.yml` with these rules as a starting point; thresholds are suggestions to tune.

| Alert | Expression idea | Severity |
|-------|-----------------|----------|
| API not ready | `up{job="sunnycloud-api"} == 0` or `/readyz` 503 for 2 min | critical |
| 5xx rate | `rate(http_requests_total{status_class="5xx"}[5m]) / rate(http_requests_total[5m]) > 0.01` for 5 min | critical |
| Disk near floor | `blob_free_bytes < 2 * blob_free_floor_bytes` | warning; below the floor: critical |
| Queue stuck | `jobs_oldest_age_seconds{kind="thumbnail"} > 600`; `{kind="transcode"} > 7200` | warning |
| Jobs failing | failed share of `jobs_total` over 1 h above 10% | warning |
| Decoder kills | `increase(decoder_kills_total[1h]) > 5` | warning |
| Scrub mismatch | `increase(scrub_mismatches_total[1d]) > 0` | critical (possible corruption) |
| Scrub stale | `time() - scrub_last_completed_timestamp_seconds > 2 * scrub_interval` | warning |
| Blob missing | `increase(scrub_missing_total[1d]) > 0` | critical |
| GC behind | `gc_oldest_due_age_seconds > 86400` | warning |
| Provider trouble | `increase(auth_provider_timeouts_total[15m]) > 3` | warning (sign-ins fail; existing sessions keep working) |
| Token reuse | `increase(auth_refresh_reuse_detected_total[1h]) > 0` | warning (security) |
| Rate-limit burst | `rate(auth_rate_limited_total[5m])` far above baseline | warning |
| Setup token left open | `auth_setup_token_pending == 1` for 1 h after install | warning (anyone with the token can become admin) |
| Migration stuck | `migrations_pending > 0` after start +5 min | critical |
| Janitor stalled | `time() - janitor_last_success_timestamp_seconds{task} > 10 * <task interval>` | warning |
| Resync wave | `increase(changes_resync_required_total[10m])` large | info (expected right after a restore) |
| Decoder image old | `time() - worker_decoder_build_timestamp_seconds > 14 * 86400` | warning (AC-63 weekly rebuild not pulled) |
| Backups | Outside sunnycloud: alert on the operator's backup tool; sunnycloud cannot see it. Keep backup interval shorter than `RETENTION` | critical |
| Containers | Restart loop of any service (cAdvisor or the Docker events feed) | warning |
| TLS | Caddy renews itself; alert on certificate expiry under 14 days from an external probe | warning |

## AC coverage

| AC | Observability element |
|----|-----------------------|
| AC-22 | `doctor` redirect-URI output, proxy and key checks, readiness |
| AC-24 | Scrub metrics and alerts; throttle sleep counter; `scrub_summary` audit event |
| AC-25 | `worker_selfcheck_ok`, `decoder_kills_total`; isolation proof is a start-up gate |
| AC-43 | `blob_read_duration_seconds`, key-check in `doctor` |
| AC-63 | `worker_decoder_build_timestamp_seconds`, `doctor` decoder age |
| AC-64 | `/api/v1/server-info`, `sync_client_versions_total` |
| AC-65 | `changes_resync_required_total`, `sync_epoch`, `migrations_pending`, audit events for restore, `doctor` epoch-marker check |
