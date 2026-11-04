# 11 HTTP API

## 1. High-Level Overview

**Purpose**: The server's only delivery layer: the hand-built hyper accept loops, routing of every `/api/v1` resource, middleware, error mapping, OpenAPI generation, ops endpoints and graceful connection shutdown. It is a library: the `sunnycloud` binary (12) builds the services, constructs `HttpServer` and runs it.

**Architectural Pattern**: Thin delivery (bind and validate, call one business operation through a `contracts` port, map the result). No business logic, no SQL and no queries in any handler.

**Single responsibility**: translate between HTTP and the business ports. It decides nothing about files, names, quota, sessions or media. The one policy it owns is protocol-level: CSRF origin rule, body-size classes, header mapping, error-to-status table.

**Crate dependencies**: `contracts` (port traits and DTOs) and helpers 01 config, 02 error, 03 clock, 04 audit, 05 ids_hashing, 08 ratelimit. It imports **no component crate** and no component imports it (enforced by `cargo deny` / a workspace graph test). The crate has no `sqlx` dependency (same test), so a query cannot be written in a handler.

## 2. Ports (declared in `contracts`)

**Provides**
| Port | Methods | Consumers |
|---|---|---|
| `HttpServer` (concrete type, not a port) | `new(ApiServices, HttpConfig)`, `run(shutdown_signal)`, `openapi()` | 12 (`serve`, `openapi` commands) |
| `ReadinessCheck` registry | collects `ReadinessCheck` items for `/readyz` | 12 registers the checks |

**Requires** (one `Arc<dyn Port>` each, handed in as `ApiServices`; names are the providers' own)
| Port | Provider | Used for |
|---|---|---|
| `Authenticator` | 01_identity | auth extraction and scope check |
| `SignInService`, `SessionService`, `DeviceGrantService`, `InviteService` | 01_identity | auth, session, device (including `DELETE /api/v1/devices/me`) and invite routes |
| `TreeService`, `TreeReader` | 02_tree | folder, node and revision routes; `TreeReader::lookup_revisions` for `POST /api/v1/revisions:lookup` |
| `UploadService`, `CommitService`, `DownloadService` | 03_transfer | transfer and content routes; derived and share downloads |
| `ChangeFeed` | 05_sync_feed | changes, head, long-poll; shutdown wake |
| `ShareService`, `PublicShareService` | 06_sharing | owner share routes; visitor routes (`open`, `verify_password`, `authorize_content`, `authorize_derived`) |
| `AdminService`, `AccountService` (`view`, `quota_view`), `AccountDeletionService` | 07_admin | admin and account routes, `GET /api/v1/accounts/me/quota` |
| `ThumbnailStateSource`, `DerivedOpener`, `WorkerAuthenticator`, `WorkerJobs` | 08_media_jobs | media and worker routes. `DerivedOpener` returns a `BlobRef` (defined in 04) that 11 passes to 03's `DownloadService::open_blob_content` |
| `VaultService` | 20_vault_service | `/api/v1/vaults/**` [v1.2] |
| `ReadinessCheck` (list) | 04, 12 and others | `/readyz` items |

`AccountGuard` (01) is called by 02 inside the commit transaction; 11 never calls it. `QuotaAdmin` (07a) is reached through 07's `AccountService`, not directly.

Business errors arrive as `AppError` (helper 02); the mapper is the only place that knows status codes.

## 3. Scope

**In**: listeners and accept loops; HTTP/2 tuning; request ID; client IP; version gate; security headers; auth extraction; CSRF; body and time limits per route class; content-serving header mapping; error mapping; route registry and OpenAPI; SPA static serving and fallback; `/healthz`, `/readyz`, `/metrics`; the test-image clock endpoints; graceful connection shutdown.
**Out**: authentication logic, sessions, abuse counters (01, 06 own counting and return `RateLimited`); sniffing and inline decision (03); idempotency and ordering (02, 03, 05); the composition root, config binding, migrations, the scheduler and janitor scheduling, construction of the scheduler and of the `AbuseLimiter` rate limiter (12 builds them; 11 receives them), and the process start/stop sequence (12); anything stateful beyond connection and shutdown state.

## 4. Listeners and Accept Loop

| Listener | Default address | Serves | Notes |
|---|---|---|---|
| public | `:8080` | `/api/v1/**`, SPA bundle | behind Caddy only |
| internal (workers) | `:8081` (`SUNNYCLOUD_INTERNAL_ADDR`) | `/api/v1/worker-jobs/**` (claim, lease, input, outputs, result, failure, status; defined by 08); in the test image also `GET /internal/v1/test/clock-offset` | backend network, worker bearer auth, plaintext, one Compose host (remote workers are out of v1.0); the public router does not contain these routes. The worker listener serves only `/api/v1/worker-jobs/**` and the test-clock read; everything else is 404, and the public listener never serves worker routes |
| ops | `:9090` | `/healthz`, `/readyz`, `/metrics` | never routed by Caddy |
| test-clock | `:9091` | `POST /test/clock` (sets the offset) | compiled only with feature `test-clock`; release builds do not contain it (404 on the public listener, FT-N-35) |

**Test clock**: the offset lives in one in-memory `OffsetState` shared by the API clock and both endpoints. `POST /test/clock` sets it; `GET /internal/v1/test/clock-offset` returns it so the supervisor's `OffsetClock` (09) and the API use the same offset (decision 10). Both exist only with feature `test-clock`.

**Single API process (decision 12)**: v1.0 runs one API process; upload progress is held in memory, so replicas are unsupported for uploads. Listeners are per process.

Each listener runs the same loop: `tokio::net::TcpListener` accept, global connection semaphore, `hyper_util::server::conn::auto::Builder` (HTTP/1.1 and h2c prior knowledge), one task per connection, `graceful_shutdown()` on stop. TLS ends at Caddy; HSTS (one year) is emitted here so other proxies get it too. **Caddy proxies to the API over h2c** (`versions h2c`); the Caddyfile must say so, otherwise the upstream hop is HTTP/1.1 and the window settings below only matter per connection. The settings below tune the Caddy-to-API hop. The *client-facing* HTTP/2 window towards browsers and apps is Caddy's, not ours, and whether it sustains 1 Gbps (AC-2) is a spike-gate item (S1), not a settled design.

| Setting (env) | Default | Reason |
|---|---|---|
| `SUNNYCLOUD_MAX_CONNECTIONS` | 4096 | AC-1 plus long-polls; excess is refused, metric `connections_rejected_total` |
| `SUNNYCLOUD_H1_HEADER_TIMEOUT` | 10s | slowloris |
| `SUNNYCLOUD_H2_STREAM_WINDOW` / `..._CONN_WINDOW` | 4MiB / 32MiB | covers 1 Gbps x 20 ms (2.5 MB) per stream; default 64 KiB caps at about 26 Mbit/s |
| `SUNNYCLOUD_H2_MAX_STREAMS` | 100 | RUSTSEC-2024-0003 class |
| `SUNNYCLOUD_H2_MAX_PENDING_RESET` / `..._MAX_HEADER_LIST` | 20 / 16KiB | rapid-reset and CONTINUATION floods |
| `SUNNYCLOUD_H2_KEEPALIVE_INTERVAL` / `..._TIMEOUT` | 30s / 20s | dead peers |

Auth is checked before any request body is read, and an unread body is dropped (its flow-control credit is released), so large windows do not become buffered memory for unauthenticated peers. Floors: tokio >= 1.54.1, h2 >= 0.4.16, bytes >= 1.11.1 (`cargo deny`).

## 5. Middleware Stack (outermost first)

| # | Layer | Behaviour |
|---|---|---|
| 1 | Panic catcher | A panic is a 500 problem body with request ID and a metric; never a dropped connection |
| 2 | Client IP | Peer in `SUNNYCLOUD_TRUSTED_PROXIES` -> use the proxy-set `X-Forwarded-For`; otherwise the peer address (default deny) |
| 3 | Request ID | New UUIDv7 per request, `X-Request-Id` on the response and in error bodies, tracing span field |
| 4 | Metrics and log | Route template label only (low cardinality); info line for non-2xx; no names, tokens or share URLs |
| 5 | Security headers | `Strict-Transport-Security` 1 year, `X-Content-Type-Options: nosniff` on **every** response including errors, `Referrer-Policy: no-referrer`, `Cache-Control: no-store` default; no CORS headers on `/api` (foreign preflight gets none); public share content may answer `Access-Control-Allow-Origin: *` without credentials |
| 6 | Timeout and body limit | By `BodyClass` (below); a declared length over the limit is 413 before a byte is read; block PUT requires `Content-Length` (411 otherwise) |
| 7 | Client version gate (AC-64) | Header `X-Sunnycloud-Client: <family>/<semver>` (`web`, `android`, `ios`, `desktop`); lower than `min_client_version` gives 426 `upgrade_required` with `min_version` and `update_url`. Public routes and `GET /api/v1/server-info` are exempt; the bundled web app always sends it. A request without the header is accepted only from a browser session (cookie); native clients must send it; a malformed value is 400 |
| 8 | Auth extraction | By the route's `AuthClass`: calls `Authenticator` (01) with the cookie and bearer, attaches `Principal{account_id, kind: Session\|Device, session_id}` or rejects. Revoked or unknown gives 401; wrong kind gives 403. Worker routes call `WorkerAuthenticator` (08) and return 403 for anything else |
| 9 | CSRF origin check (AC-58) | Applies to cookie-authenticated POST, PUT, PATCH, DELETE: allow only `Sec-Fetch-Site: same-origin` or an `Origin` equal to the configured public origin; cross-site, same-site, sibling subdomain, or neither header gives 403 `csrf_rejected`. Bearer-authenticated requests are exempt. GET/HEAD never mutate (checked by a generated test) |
| 10 | Handler | bind -> validate -> one port call -> map |

**Rate limits**: no generic limiter in v1.0. AC-20 limits are counted at the failure site (01 for OAuth steps and device approvals, 06 for share passwords and invalid tokens) through the shared in-memory `AbuseLimiter` (helper 08 ratelimit), which 12 constructs once and injects into those components (and into 11 where it needs it). 11 only maps `RateLimited{retry_after}` to 429 with `Retry-After`.

**AuthClass** (declared per route in the registry): `Public`; `Session` (cookie or session bearer); `SessionOrDevice` (sync scope: transfer, tree reads and writes, changes); `DeviceOnly` (`DELETE /api/v1/devices/me`: a device token removes itself; sessions use `DELETE /devices/{id}`); `SessionOnly` (approve-device, create-share, delete-account, link-provider, device revoke by id: device tokens get 403, AC-57); `Admin` (session with role admin); `ShareGrant` (06 resolves token or access grant); `Worker` (internal listener).

**BodyClass**: `Small` 64 KiB (JSON default); `Commit` 4 MiB (`commits`, blocklist up to about 1.1 MB base64 for 50 GiB); `Batch` 4 MiB (500 entries); `Block` exactly the block length for that index, at most 2 MiB; `Staged` at most 2 MiB; `Query` 128 KiB (`thumbnail-states`); `Auth` 16 KiB; `WorkerOutput` per-kind cap from 08 (`derived_max_*`); `None` for GET. Idle body timeout 30 s for `Block`/`Staged`; `LongPoll` allows `wait` + 5 s; downloads have a write-idle timeout (60 s) and no total cap; default request timeout 60 s.

## 6. Error Mapping

Business errors implement `AppError { kind: ErrorKind, code: &'static str, detail: Option<safe text>, extra }` (helper 02). Central mapper:

| Kind | Status | Kind | Status |
|---|---|---|---|
| Validation | 400 (422 for semantic) | Gone (`resync_required`, expired session) | 410 |
| Unauthorized | 401 | PreconditionFailed (`need_blocks`, `edited_since`) | 409/412 per code |
| Forbidden | 403 | PayloadTooLarge | 413 |
| NotFound (also foreign id) | 404 | RangeNotSatisfiable | 416 |
| Conflict (`name_conflict`, `block_conflict`) | 409 | UpgradeRequired (`upgrade_required`) | 426 |
| RateLimited | 429 + `Retry-After` | Unavailable (DB down, overload) | 503 + `Retry-After` |
| Internal (unexpected) | 500 opaque | QuotaExceeded (`quota_exceeded`) | 507 Insufficient Storage, nothing reserved |

Body is `application/problem+json`: `type` (`urn:sunnycloud:error:<code>`), `title`, `status`, `code`, `detail`, `request_id`, plus typed extras (`need_blocks`, `retry_after`, `min_version`). Unexpected errors are logged once with the full chain and request ID; the client sees only the opaque body. A stream error after headers aborts the stream (RST_STREAM or close), never a truncated clean response. After the database returns the pool reconnects on its own and the server answers within 60 s.

## 7. Content-Serving Headers

One function maps a `Download` (03; for derived files the `BlobRef` from 08 goes through 03's `DownloadService`) to headers, used by node/revision content, share content, thumbnails and transcodes: `Content-Type` (the allowlisted type when `Inline`, else `application/octet-stream`), `Content-Disposition` (`inline`, or `attachment; filename*=UTF-8''...` from `suggested_name`, sanitised), `X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox` on attachments, `Accept-Ranges: bytes`, `ETag`, `Content-Range`, status 200/206/304/416, `Cache-Control: private`. Error responses on these routes keep `nosniff` (layer 5). The inline decision itself is 03's (AC-47).

## 8. Route Registry (single source for router, OpenAPI, CSRF and IDOR generators)

`route!` registers: path, method, owner port method, `AuthClass`, `BodyClass`, `x-id-params` (every ID parameter, for the AC-59 matrix), state-changing flag (for the AC-58 matrix). A unit test compares the router with the generated OpenAPI: 0 undocumented routes. Paths are indicative for components other than 03, 07 and 08; the owning component's spec is authoritative.

| Group (owner) | Resources | Auth |
|---|---|---|
| System (11) | `GET /api/v1/server-info` returns `ServerInfo` (below); `GET /api/v1/openapi.json`; ops listener; SPA fallback for non-`/api` paths | Public |
| Auth (01) | `GET /api/v1/auth/providers`, `.../providers/{provider}/login`, `.../callback`, `POST /api/v1/auth/code-exchanges` (mobile one-time code), session refresh, logout | Public / Session |
| Sessions and devices (01) | `GET /api/v1/sessions`, `DELETE .../{id}`, `POST /api/v1/device-grants` (daemon), device-token poll, `GET/POST /api/v1/device-grants/{user_code}` (approve/deny), `GET /api/v1/devices`, `DELETE /api/v1/devices/{id}` (session), `DELETE /api/v1/devices/me` (device removes itself; 01 `DeviceGrantService::revoke_device`) | Public (daemon) / SessionOnly / DeviceOnly |
| Account (07, 01) | `GET /api/v1/accounts/me` (incl. root node id), `DELETE /api/v1/accounts/me`, `GET /api/v1/accounts/me/quota` (07); link identity, `GET /api/v1/setup` (01) | Session / SessionOnly / Public |
| Tree (02) | `/api/v1/nodes` (create folder), `/nodes/{id}`, `/nodes/{id}/children` (keyset paging), rename/move, delete, `/revisions/{id}`, `POST /api/v1/revisions:lookup` (body up to 1,000 content hashes; `TreeReader::lookup_revisions`; exists only for reinstall rebuild, there is no commit-by-hash and no `server_has_hashes`) | SessionOrDevice |
| Transfer (03) | `/api/v1/uploads`, `/uploads/{id}`, `/uploads/{id}/blocks/{index}`, `/staged-uploads/{key}`, `/commits`, `/commits:batch`, `/nodes/{id}/content`, `/revisions/{id}/content` (current revisions only for non-admin principals; a retained non-current revision is restorable by the operator but not downloadable by users: 404) | SessionOrDevice |
| Sync (05) | `GET /api/v1/changes?cursor&wait&limit`, `GET /api/v1/changes/head` | SessionOrDevice (long-poll) |
| Sharing (06) | `/api/v1/shares` (create is SessionOnly), `/shares/{id}`; visitor routes `/api/v1/public-shares/{token}`, `.../access`, `.../content`, `.../thumbnail`. The thumbnail route: 06 `PublicShareService::authorize_derived` checks token and grant and returns the `LiveFile`; 11 then calls 08 `DerivedOpener::open_derived_for(live_file, kind)` and streams the `BlobRef` through 03's `DownloadService`. Nothing in 08 authorises visitors | Session / ShareGrant / Public |
| Admin (07, 01) | `/api/v1/admin/users`, `.../users/{id}`, `.../users/{id}/quota` (07), `/admin/invites` (01), `/admin/health` (07) | Admin |
| Media (08) | `/api/v1/nodes/{id}/thumbnail`, `/nodes/{id}/transcode`, `POST /api/v1/thumbnail-states` | SessionOrDevice |
| Worker (08, internal listener) | `POST /api/v1/worker-jobs/claims`, `PUT\|DELETE /api/v1/worker-jobs/{job_id}/lease`, `GET .../input`, `PUT .../outputs/{kind}`, `PUT .../result`, `POST .../failure`, `PUT /api/v1/worker-jobs/status` | Worker |
| Vault (20, v1.2) | `/api/v1/vaults/**` reserved | Session |

**`ServerInfo`** (defined here, returned by `GET /api/v1/server-info`, built by 12 from the typed config sections and the build): `{block_size, max_lanes, small_file_max, batch_max_entries, max_staged_per_account, longpoll_max_wait, min_client_version, api_versions}`. `block_size` (2 MiB) and `small_file_max` (the staged-upload cap, 2 MiB) are protocol constants; `max_lanes`, `batch_max_entries`, `max_staged_per_account` and `longpoll_max_wait` are the values of the config keys `max_lanes`, `batch_max_entries`, `max_staged` and `longpoll_wait_max`; `min_client_version` is the configured minimum; `api_versions` lists the served `/api/vN` families. Providers and the setup flag are not part of it: clients take them from `GET /api/v1/auth/providers` and `GET /api/v1/setup` (01). The info route is exempt from the version gate so an old client can read `min_client_version` and show the upgrade prompt.

Each handler module (`routes/auth.rs`, `devices.rs`, `accounts.rs`, `nodes.rs`, `transfer.rs`, `changes.rs`, `shares.rs`, `admin.rs`, `media.rs`, `internal.rs`, `ops.rs`) stays under 500 lines. REST naming: plural lowercase kebab nouns, no trailing slash, filters as query parameters; the only verb form is `commits:batch`, fixed by the solution. SPA pages never collide with `/api`.

## 9. OpenAPI

`utoipa` annotations on handlers and `ToSchema` on wire DTOs (versions pinned at implementation, **[Verify at pin]**). The document is emitted by `sunnycloud openapi` (12) through `HttpServer::openapi()` and served read-only. CI: (1) diff against the last release tag fails on removed or tightened operations and fields (additive-only, AC-64); (2) AC-58 CSRF matrix and AC-59 two-account matrix are generated from it and fail when an operation has no test; (3) every operation declares `AuthClass` and `x-id-params`. Security schemes: cookie session, bearer session, bearer device, bearer worker.

## 10. Ops Endpoints

- `/healthz`: process answers; no dependency checks. `/readyz`: 200 or 503 with a JSON list built from the registered `ReadinessCheck`s: database reachable, migrations current, key check passed, blob volume writable and above the floor, scheduler alive (check registered by 12); body `status: ok|degraded` (degraded when 04 reports suspect blobs); **503 when any contributor is degraded** (or failing), 200 only when all are ok. `/metrics`: Prometheus text; HTTP and accept-loop metrics here (`http_requests_total`, `http_request_duration_seconds`, `connections_open`, `connections_rejected_total{reason}`, `body_limit_exceeded_total`); components register their own collectors.
- **Shutdown of connections**: on the signal passed to `run`, readiness turns 503, accepting stops, held long-polls are woken with an empty page (`ChangeFeed` shutdown signal), `graceful_shutdown()` runs on connections and waits up to `SUNNYCLOUD_SHUTDOWN_GRACE` (25 s). The ordering of the rest of the process shutdown (scheduler, upload checkpoints, group sync, pool) belongs to 12.

## 11. Configuration (selected; typed `HttpConfig` bound by 12 through helper 01)

`SUNNYCLOUD_PUBLIC_URL`, `SUNNYCLOUD_TRUSTED_PROXIES`, `SUNNYCLOUD_INTERNAL_ADDR`, the accept-loop table above, `SUNNYCLOUD_SHUTDOWN_GRACE`, `SUNNYCLOUD_MIN_CLIENT_VERSION` (one semver, or `family=semver,...`), `SUNNYCLOUD_REQUEST_TIMEOUT` (60s), `SUNNYCLOUD_BODY_IDLE_TIMEOUT` (30s). Every duration is a config value; tests use short ones.

## 12. Security Notes

Internal routes exist only in the internal router (AC-79). Cookies are `__Host-`, `Secure`, `HttpOnly`, `SameSite=Lax` (set from the DTO 01 returns). Host header is passed to 01 for the callback check (`redirect_uri` comes from config). No request body, token, name or share URL reaches logs. Stalled-connection exposure is bounded by connection and stream caps and header timeouts (NFT-SEC-15). Errors never leak SQL, paths or keys. The test-clock endpoints are absent from release builds.

## 13. ACs Implemented

| AC | Release | Part |
|---|---|---|
| AC-58 | v1.0 | CSRF layer + generated matrix |
| AC-59 | v1.0 | Registry-driven IDOR matrix; stalled connection caps |
| AC-64 | v1.0 | Version gate, additive-only OpenAPI diff |
| AC-47 | v1.0 | Header mapping, `nosniff` everywhere |
| AC-20 (mapping) | v1.0 | 429 + `Retry-After` |
| AC-21 (part) | v1.0 | 401 on revoked, shutdown wake of long-polls |
| AC-2 | v1.0 | H2 windows keep 1 Gbps reachable |
| AC-22/AC-65 (part) | v1.0 | Readiness reflects migrations and key check |

**Interaction ACs folded in**: AC-79 (internal listener, uniform 403, 404 on public), AC-69 (no handler code runs between commit return and response mapping that can reorder; wake-up after commit is 05's).

## 14. Tests

FT-N-27, FT-N-35, FT-P-70, FT-P-05, NFT-SEC-09, NFT-SEC-10 (cookie parts), NFT-SEC-11, NFT-SEC-13, NFT-SEC-14, NFT-SEC-15, NFT-SEC-18, NFT-SEC-19, NFT-RES-LIM-02, NFT-RES-20, NFT-RES-21, NFT-PERF-02, NFT-PERF-03, NFT-PERF-10 (listing latency through the stack), NFT-PERF-30. Step 5: AC-79 through the public proxy; `GET /internal/v1/test/clock-offset` present in the test image and absent from the release image. Unit: error table, CSRF matrix, header mapper over 7 inline and 6 hostile types, body-class limits, route/OpenAPI parity, graceful shutdown ordering of listener steps, crate graph test (no component crate, no sqlx).

## 15. Acceptance for the Component

0 undocumented routes; CSRF 403 on N of N state-changing operations; 1,000 stalled connections add <= 200 MB and slow a 1 GiB upload <= 10%; a single transfer reaches 90% of 1 Gbps through the stack; SIGTERM during 100 uploads loses no acked block; coverage 90% on CSRF, auth gating, error mapping and header mapping (security-critical).

## 16. Risks
**Defaults chosen at Step 5 gap pass**: `/readyz` is 503 when any contributor is degraded (so suspect blobs from 04 also make it 503); `X-Sunnycloud-Client` may be absent only on a browser-session request, native clients must send it, malformed gives 400; the worker listener `:8081` serves only `/api/v1/worker-jobs/**` and the test-clock read (everything else 404), and the public listener never serves worker routes.


| Risk | Mitigation |
|---|---|
| Caddy hop is HTTP/1.1 so window tuning is moot | Caddyfile uses `versions h2c` (change to `containerization.md`); client-facing window behaviour at 1 Gbps is spike-gate S1 |
| Large H2 windows raise memory under attack | Auth before body, caps, NFT-SEC-15 gate; reduce windows first if it fails |
| Route registry drifts from handlers | Parity test in CI |
| Business logic creeping into handlers | One port call per handler; crate graph test; review rule |
| Single API process | Documented limit of v1.0 (in-memory upload progress); janitor cycles stay claim-safe |

## 17. Dependency Graph

After: `contracts` and the helpers. Parallel with: component implementations (unit tests of 11 use fakes of the ports). Blocks: 12 `serve`, clients' integration, blackbox tests, release.
