# Test Specification — 11 HTTP API

Scope: the `HttpServer` public surface (`new(ApiServices, HttpConfig)`, `run(shutdown_signal)`, `openapi()`), the `ReadinessCheck` registry, the four listeners (public `:8080`, internal `:8081`, ops `:9090`, test-clock `:9091`), the middleware stack, error mapping, content-serving headers, route registry/OpenAPI, `ServerInfo`. 11 holds no business logic, no SQL and imports no component crate, so two harness modes are used:

- **Mode S (stub services)**: an in-process `HttpServer` built with `ApiServices` whose ports are test stubs that return scripted results (needed to provoke each `ErrorKind`, panics, stream errors, slow bodies; no database is involved, so no query-correctness is at stake). Real hyper client over TCP/h2c, not a function call.
- **Mode R (real stack)**: the Compose test stack (real components, PostgreSQL, Caddy), driven like a client. System scenarios are cited by ID, not restated.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-58 | Cross-origin cookie POST returns 403; no Origin and no Sec-Fetch-Site returns 403; refresh races | IT-11-6, ST-11-1; sys NFT-SEC-09, NFT-SEC-10 | Covered by this file (CSRF layer, matrix from OpenAPI) and system scenarios (refresh race is 01) |
| AC-59 | Registry-driven two-account matrix; stalled connections bounded | IT-11-10, IT-11-11, IT-11-19, PT-11-2, ST-11-3; sys NFT-SEC-13, NFT-SEC-14, NFT-SEC-15, NFT-RES-LIM-02 | Covered by this file and system scenarios |
| AC-64 | Version gate, additive-only OpenAPI diff | IT-11-7, IT-11-8, IT-11-12; sys FT-N-27, FT-P-70 | Covered by this file and system scenarios |
| AC-47 | Header mapping and `nosniff` everywhere | IT-11-5, IT-11-14; sys FT-N-11, FT-P-39, NFT-SEC-19 | Covered by this file and system scenarios |
| AC-20 | 429 + `Retry-After` mapping | IT-11-1, IT-11-15; sys NFT-SEC-04, NFT-SEC-05, NFT-SEC-06 | Covered by this file (mapping) and system scenarios (counting is 01/06) |
| AC-21 | 401 on revoked; shutdown wake of long-polls | IT-11-10, IT-11-16; sys FT-N-30, NFT-RES-26 | Covered by this file and system scenarios |
| AC-2 | H2 windows keep 1 Gbps reachable | IT-11-18, PT-11-1; sys NFT-PERF-02, NFT-PERF-03 | Covered by this file (settings and an isolated stream) and sys NFT-PERF-02/03 (`C-REF`, gate S1) |
| AC-22 | Readiness reflects migrations and key check | IT-11-17; sys FT-P-69, NFT-RES-14 | Covered by this file and system scenarios |
| AC-65 | Readiness during upgrade/restore | IT-11-17; sys FT-N-39 | Covered by this file and system scenarios |
| AC-79 | Internal listener separation, uniform 403, 404 on public (listener side) | IT-11-9, ST-11-4; sys NFT-SEC-50 | Covered by this file and sys NFT-SEC-50 (through Caddy) |
| AC-69 | No handler step between commit return and response mapping can reorder | IT-11-2; sys NFT-RES-35 | Covered for the response mapping; ordering itself is 05 and 02, sys NFT-RES-35 |
| AC-1 | 4096 connections for scale | IT-11-19; sys NFT-PERF-01, NFT-RES-21 | Covered by this file (connection cap) and system scenarios |
| AC-38 | SIGTERM during 100 uploads loses no acked block (component acceptance) | AT-11-2; sys NFT-RES-03, NFT-RES-22 | Covered by this file and system scenarios |
| AC-57 | Device tokens get 403 on session-only routes | IT-11-10; sys NFT-SEC-08 | Covered by this file and sys NFT-SEC-08 |

## Blackbox Tests

Default Mode S config: `SUNNYCLOUD_PUBLIC_URL=https://sunnycloud.test`, `SUNNYCLOUD_REQUEST_TIMEOUT=2s`, `SUNNYCLOUD_BODY_IDLE_TIMEOUT=1s`, `SUNNYCLOUD_H1_HEADER_TIMEOUT=1s`, `SUNNYCLOUD_SHUTDOWN_GRACE=2s`, `SUNNYCLOUD_MAX_CONNECTIONS=50`, `SUNNYCLOUD_MIN_CLIENT_VERSION=1.2.0`, `SUNNYCLOUD_TRUSTED_PROXIES=10.0.0.0/24`. Cases state their overrides.

### IT-11-1: Error table
**Summary**: each `ErrorKind` maps to its status with a `application/problem+json` body.
**Traces to**: AC-20, AC-64
**Input data**: Mode S; one stub route per kind returning `AppError`: Validation (400 and the semantic 422 variant), Unauthorized, Forbidden, NotFound, Conflict (`name_conflict`, `block_conflict`), RateLimited{retry_after 17}, Gone (`resync_required`, expired session), PreconditionFailed (`need_blocks`, `edited_since`), PayloadTooLarge, RangeNotSatisfiable, UpgradeRequired, Unavailable, QuotaExceeded, Internal.
**Expected result**: 400/422, 401, 403, 404, 409, 429, 410, 413, 416, 426, 503, 507, 500; body JSON keys `type = urn:sunnycloud:error:<code>`, `title`, `status` (equal to the HTTP status), `code`, `detail`, `request_id` (equal to the `X-Request-Id` header) and typed extras (`need_blocks`, `retry_after`, `min_version`); 429 and 503 carry `Retry-After`; 507 code `quota_exceeded`. The 409-versus-412 choice for `need_blocks` and `edited_since` is BLOCKED (description says "409/412 per code" without assigning): the test asserts only that each is 409 or 412 and records which (gap G-11-1).
**Max execution time**: 10 s
**Dependencies**: Mode S

### IT-11-2: Internal errors are opaque; success bodies untouched
**Summary**: unexpected errors reveal nothing and are logged once; success bodies are the port result verbatim.
**Traces to**: AC-69
**Input data**: stub returns `Internal` whose chain contains `SELECT * FROM nodes WHERE ...`, `/var/lib/sunnycloud/blobs/ab/cd/x`, and a fake key `K3Y-MARK`; 50 concurrent stub commits that return increasing `seq` values after random delays of 0 to 50 ms.
**Expected result**: 500 body has no SQL, path or marker; one ERROR log line with the full chain and the same `request_id`; each commit response body contains exactly the `seq` its stub call returned (0 mismatches over 1,000 calls); handlers add no await between port return and mapping (response order equals completion order of each stub, not reordered).
**Max execution time**: 20 s
**Dependencies**: Mode S

### IT-11-3: Panic catcher
**Summary**: a panic becomes a 500 problem body and the connection survives.
**Traces to**: AC-47
**Input data**: stub handler panics; two requests on the same h2 connection (panic then GET `server-info`).
**Expected result**: first returns 500 with `request_id` and problem body; second returns 200 on the same connection (no connection reset); metric for panics incremented by 1; `nosniff` present.
**Max execution time**: 5 s
**Dependencies**: Mode S

### IT-11-4: Request ID
**Summary**: every response carries a unique UUIDv7 request id.
**Traces to**: AC-47
**Input data**: 1,000 requests: success, 404, 401, 413, 426, 500, OPTIONS, HEAD.
**Expected result**: `X-Request-Id` present on every response, parses as UUID version 7, all 1,000 distinct; error bodies repeat it; a client-supplied `X-Request-Id` is not echoed as the id.
**Max execution time**: 10 s
**Dependencies**: Mode S

### IT-11-5: Security headers on every response
**Summary**: fixed headers are present on all responses including errors; CORS is closed.
**Traces to**: AC-47
**Input data**: 200, 204, 304, 400, 401, 403, 404, 405, 413, 416, 426, 429, 500, 503 responses; `OPTIONS /api/v1/nodes` with `Origin: https://evil.test` and `Access-Control-Request-Method: POST`; the public share content route.
**Expected result**: every response has `Strict-Transport-Security: max-age=31536000`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store` (except content routes: `private`); no `Access-Control-*` header on `/api` including the preflight; the share content route may answer `Access-Control-Allow-Origin: *` and never sends `Access-Control-Allow-Credentials`.
**Max execution time**: 10 s
**Dependencies**: Mode S

### IT-11-6: CSRF origin rule
**Summary**: cookie-authenticated mutations require same-origin evidence; bearer requests are exempt.
**Traces to**: AC-58
**Input data**: matrix generated from `HttpServer::openapi()`: every operation flagged state-changing, with a valid session cookie, over header sets: `Origin` = foreign, same-site sibling subdomain, `https://sunnycloud.test` (public URL), `null`; `Sec-Fetch-Site` = cross-site, same-site, same-origin, none; neither header; plus bearer-authenticated requests with a foreign Origin; every GET/HEAD route with cross-site headers.
**Expected result**: foreign, sibling, `null`, cross-site, same-site, and no-header cookie requests get 403 `csrf_rejected` on N of N operations (N printed in the report); `Origin` equal to the public URL or `Sec-Fetch-Site: same-origin` passes the layer (stub returns 2xx); bearer requests pass; all GET/HEAD routes leave a stub state counter unchanged; a mutation operation missing from the generated list fails the generator.
**Max execution time**: 60 s
**Dependencies**: Mode S, OpenAPI

### IT-11-7: Client version gate
**Summary**: old clients get 426 `upgrade_required` with the minimum version and update URL.
**Traces to**: AC-64
**Input data**: `X-Sunnycloud-Client`: `web/1.2.0`, `android/1.1.9`, `desktop/1.2.0`, `desktop/1.3.0`, `ios/0.9.0`; config variants `1.2.0` and `web=1.0.0,android=1.5.0`; routes: a protected route, a public route, `GET /api/v1/server-info`.
**Expected result**: `android/1.1.9` -> 426 with body `code: upgrade_required`, `min_version` and `update_url`; equal or higher passes; family-specific minimums apply per family; public routes and `server-info` pass for `ios/0.9.0`; the gate runs before auth (a 426 for an unauthenticated old client, not 401). Absent header and malformed values (`foo`, `web/x.y`) are BLOCKED: the behaviour is not specified (gap G-11-2).
**Max execution time**: 10 s
**Dependencies**: Mode S

### IT-11-8: ServerInfo
**Summary**: `GET /api/v1/server-info` returns exactly the specified fields from config.
**Traces to**: AC-64
**Input data**: HttpConfig built from non-default values: `max_lanes=3`, `batch_max_entries=250`, `max_staged=7`, `longpoll_wait_max=20s`, `min_client_version=1.2.0`; request unauthenticated, then with `X-Sunnycloud-Client: web/0.1.0`.
**Expected result**: JSON keys exactly `block_size`, `max_lanes`, `small_file_max`, `batch_max_entries`, `max_staged_per_account`, `longpoll_max_wait`, `min_client_version`, `api_versions`; `block_size = 2097152`, `small_file_max = 2097152`, `max_lanes = 3`, `batch_max_entries = 250`, `max_staged_per_account = 7`, `longpoll_max_wait` equals 20 s in the schema's unit, `min_client_version = "1.2.0"`, `api_versions = ["v1"]`; no provider list and no setup flag; 200 with the old client header (exempt); body validates against the OpenAPI schema; `Cache-Control: no-store`.
**Max execution time**: 5 s
**Dependencies**: Mode S, OpenAPI

### IT-11-9: Listeners and route placement
**Summary**: worker routes exist only on `:8081`; test routes exist only in the test image.
**Traces to**: AC-79
**Input data**: Mode R (test image and release image): requests to `:8080`, `:8081`, `:9090`, `:9091`, and through Caddy.
**Expected result**: `POST /api/v1/worker-jobs/claims`, `GET .../{id}/input`, `PUT .../status` on `:8080` and through Caddy: 404 (same body as an unknown path); on `:8081` without or with non-worker credentials (cookie, session bearer, device bearer): 403 identical; with the worker bearer: reaches the handler (204/409/404 per state); `/healthz`, `/readyz`, `/metrics` answer on `:9090` only (404 on `:8080`); test image: `POST :9091/test/clock` works and `GET :8081/internal/v1/test/clock-offset` returns the offset; release image: connection to `:9091` refused, `/internal/v1/test/clock-offset` 404, `/test/clock` 404 on the public listener (FT-N-35). Reverse isolation (public routes on `:8081`) is not specified (gap G-11-3).
**Max execution time**: 60 s
**Dependencies**: Mode R, both images

### IT-11-10: Auth classes
**Summary**: every `AuthClass` accepts and rejects the right principals.
**Traces to**: AC-57, AC-59, AC-21
**Input data**: routes from the registry grouped by `AuthClass` (`Public`, `Session`, `SessionOrDevice`, `DeviceOnly`, `SessionOnly`, `Admin`, `ShareGrant`, `Worker`); principals: none, session, device, admin session, non-admin session, revoked session, revoked device, expired session.
**Expected result**: no credential on a protected route 401; revoked or unknown 401; wrong kind 403 (device token on approve-device, create-share, delete-account, link-provider, device revoke by id; session on `DELETE /devices/me`; non-admin on admin); `Public` accepts everyone; auth runs before the body is read; the principal reaches the stub as `Principal{account_id, kind, session_id}` with the right kind.
**Max execution time**: 30 s
**Dependencies**: Mode S

### IT-11-11: Route registry and OpenAPI parity
**Summary**: router and OpenAPI describe the same operations; naming rules hold.
**Traces to**: AC-59, AC-64
**Input data**: `HttpServer::openapi()`, the live router route table, `GET /api/v1/openapi.json`.
**Expected result**: set of (method, path template) in the router equals the set in OpenAPI (0 undocumented routes, 0 phantom operations); every operation declares `AuthClass`, `x-id-params` (every ID parameter) and the state-changing flag; the served document equals `openapi()` byte for byte; paths: lowercase kebab plural nouns, no trailing slash, only `commits:batch` and `revisions:lookup` use a verb form, no collection on SPA page routes, no route outside `/api` except ops and SPA fallback; non-`/api` paths fall back to the SPA index.
**Max execution time**: 10 s
**Dependencies**: Mode S

### IT-11-12: Additive-only OpenAPI diff gate
**Summary**: the CI diff rule rejects removals and tightening.
**Traces to**: AC-64
**Input data**: last release document (fixture) versus modified copies: operation removed, required field added to a request, enum value removed, response field removed, new optional field added, new operation added.
**Expected result**: the first four fail the diff tool; the last two pass.
**Max execution time**: 10 s
**Dependencies**: OpenAPI fixtures (CI)

### IT-11-13: Body classes and timeouts
**Summary**: size and time limits are enforced per route class before the body is read.
**Traces to**: AC-59
**Input data**: `Small` 64 KiB + 1, `Commit` 4 MiB + 1, `Auth` 16 KiB + 1, `Query` 128 KiB + 1, `Block` 2 MiB + 1 and without `Content-Length`, chunked body without declared length beyond the limit; headers-only request declaring 10 GiB; body sent 1 byte per 500 ms on a `Block` route (idle 1 s); a request outlasting 2 s; a long-poll with `wait` + 5 s.
**Expected result**: declared oversize -> 413 within 100 ms with 0 body bytes consumed (`body_limit_exceeded_total` +1); block PUT without `Content-Length` -> 411; chunked oversize aborted at the limit; slow body -> request ends at idle timeout (about 1 s) with 408 or reset; default request timeout 2 s applies; long-poll allowed up to `wait` + 5 s then answered; `None` class GET with a body is rejected or ignored without buffering.
**Max execution time**: 60 s
**Dependencies**: Mode S

### IT-11-14: Content-serving header mapper
**Summary**: one function maps a `Download` to headers for node, revision, share, thumbnail and transcode routes.
**Traces to**: AC-47
**Input data**: Mode S `DownloadService` stub returning 7 inline types (jpeg, png, gif, webp, avif, mp4, webm) and 6 hostile types (html, svg, xml, js, pdf, polyglot) with `suggested_name` values including `a"b.txt`, CR/LF, `..\x`, bidi U+202E, a 300-byte UTF-8 name; Range `bytes=0-0`, `bytes=5-`, `bytes=-3`, `bytes=99999999-`, `If-None-Match` equal to the ETag.
**Expected result**: inline types: exact `Content-Type`, `Content-Disposition: inline`, `nosniff`; hostile types: `application/octet-stream`, `attachment; filename*=UTF-8''...` with the name sanitised (no CR/LF, quotes escaped, no path separators or bidi controls), `Content-Security-Policy: sandbox`; all: `Accept-Ranges: bytes`, `ETag`, `Cache-Control: private`; 206 with correct `Content-Range`, 416 for an unsatisfiable range with `Content-Range: bytes */len`, 304 for matching ETag; error responses on these routes keep `nosniff`; 0 inline renders for hostile types (7 inline / 6 hostile table).
**Max execution time**: 15 s
**Dependencies**: Mode S

### IT-11-15: Rate-limit and unavailable mapping, client IP
**Summary**: `RateLimited` and `Unavailable` carry `Retry-After`; the client IP comes from trusted proxies only.
**Traces to**: AC-20
**Input data**: stub returns `RateLimited{retry_after: 17}` and records the client IP it was given; requests from a peer inside `10.0.0.0/24` with `X-Forwarded-For: 198.51.100.7`, from an outside peer with the same header, and with a spoofed header list `1.1.1.1, 198.51.100.7`.
**Expected result**: 429 with `Retry-After: 17` and `retry_after: 17` in the body; trusted peer -> stub sees `198.51.100.7`; untrusted peer -> stub sees the peer address (header ignored, default deny); rotating `X-Forwarded-For` from an untrusted peer does not change the identity used for limiting.
**Max execution time**: 10 s
**Dependencies**: Mode S

### IT-11-16: Graceful shutdown
**Summary**: shutdown order is readiness 503, stop accepting, wake long-polls, drain, close.
**Traces to**: AC-21, AC-38
**Input data**: 20 held long-polls (stub `ChangeFeed` honours the shutdown signal), 3 downloads in progress (one finishing within 1 s, one needing 5 s), then the `run` shutdown signal; `SHUTDOWN_GRACE=2s`.
**Expected result**: timestamps in order: `/readyz` 503 < new connection refused or closed < long-polls answered with an empty page (200, no cursor loss) < connections closed; the short download completes with the full body; the long one is closed at grace end (about 2 s) with a stream reset, not a clean truncated response; `run` returns within grace + 1 s.
**Max execution time**: 20 s
**Dependencies**: Mode S

### IT-11-17: Ops endpoints and readiness
**Summary**: `/healthz` has no dependency checks; `/readyz` aggregates registered checks; metrics are low-cardinality.
**Traces to**: AC-22, AC-65
**Input data**: Mode S with `ReadinessCheck`s database, migrations current, key check, blob volume, scheduler alive; flip each to failing in turn; a suspect-blob check returning degraded; Mode R: stop `db`, restart, then upgrade-interrupted state (migration pending); 1,000 requests to `/api/v1/nodes/{id}` with distinct ids.
**Expected result**: all ok -> `/readyz` 200 `status: ok`; each failing check -> 503 with a JSON list naming that item and only failing detail (no SQL or paths); degraded -> body `status: degraded` (HTTP code recorded, gap G-11-4); `/healthz` 200 even with the database down; Mode R: `/readyz` 503 during DB outage and back to 200 within 60 s of the database accepting connections; pending migration -> 503 naming migrations; `/metrics` Prometheus text contains `http_requests_total`, `http_request_duration_seconds`, `connections_open`, `connections_rejected_total`, `body_limit_exceeded_total`; the 1,000 ids produce one series (route template label) and no label holds a token, name or share URL.
**Max execution time**: 120 s
**Dependencies**: Mode S and Mode R

### IT-11-18: HTTP/2 settings and h2c
**Summary**: advertised h2 settings and the upstream mode match the configuration.
**Traces to**: AC-2
**Input data**: raw h2c client reads the server SETTINGS and WINDOW_UPDATE frames; defaults and overridden values.
**Expected result**: `SETTINGS_INITIAL_WINDOW_SIZE = 4194304`, connection window 33554432 (32 MiB) after the first WINDOW_UPDATE, `MAX_CONCURRENT_STREAMS = 100`, `MAX_HEADER_LIST_SIZE = 16384`; h2c prior knowledge and HTTP/1.1 both served on the same port; keepalive pings sent at the configured interval (shortened for the test) and a peer that never answers is closed after the timeout.
**Max execution time**: 30 s
**Dependencies**: Mode S

### IT-11-19: Connection limits and abusive peers
**Summary**: caps, header timeouts and rapid-reset protection hold.
**Traces to**: AC-59, AC-1
**Input data**: 60 connections with `MAX_CONNECTIONS=50`; slowloris headers on h1 (1 byte per 300 ms, timeout 1 s); 1,000 `HEADERS` + `RST_STREAM` pairs in 1 s on one h2 connection (`MAX_PENDING_RESET=20`); a CONTINUATION flood; a header list of 17 KiB; unauthenticated 2 MiB `PUT` of a block (body ignored).
**Expected result**: connections 51 to 60 refused or closed, `connections_rejected_total` +10; slowloris closed within 2 s; rapid reset -> GOAWAY and close; CONTINUATION flood and oversize header list rejected (4xx/431 or stream reset, never 2xx); the unauthenticated PUT gets 401/403 immediately, its body is dropped and flow-control credit released (server RSS grows under 10 MB over 100 such requests).
**Max execution time**: 60 s
**Dependencies**: Mode S

### IT-11-20: Streams abort on error; cookies come from the DTO
**Summary**: a stream error after headers never ends as a clean truncated response; cookie attributes are as given.
**Traces to**: AC-47
**Input data**: stub download stream with `Content-Length: 10 MiB` that errors after 3 MiB (h2 and h1); stub sign-in returning a session cookie DTO.
**Expected result**: h2: RST_STREAM; h1: connection closed before `Content-Length` bytes (client sees an error, not EOF-success); `Set-Cookie` equals `__Host-` name, `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, no `Domain`; the server never invents attributes.
**Max execution time**: 10 s
**Dependencies**: Mode S

## Performance Tests

### PT-11-1: Single stream through the stack
**Summary**: one h2c stream reaches 90% of 1 Gbps with a memory-backed port stub (isolates 11 from disk).
**Traces to**: AC-2
**Load scenario**:
- Concurrent users: 1 stream, then 4 streams
- Request rate: one 5 GiB GET and one 5 GiB PUT (discarded) per run
- Duration: 2 minutes per direction
- Ramp-up: none; `shape-perf`, RTT 20 ms, perf-client on REF-LOAD

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Throughput | 117 MB/s | 112 MB/s |
| Server CPU | 1 core or less | 2 cores |

**Resource limits**:
- Memory growth under 64 MiB. `C-REF` only; elsewhere `NOT-RUN(HW-REF)`. Calibrated goodput is recorded. Real-disk and encryption cases are sys NFT-PERF-02/03.

### PT-11-2: Stalled connections and request overhead
**Summary**: 1,000 stalled TLS/h2 connections do not move memory or delay an upload; routing overhead is small.
**Traces to**: AC-59
**Load scenario**:
- DS-30: 1,000 connections that handshake and send nothing (`perf-client stall --count 1000`), while a 1 GiB PUT (DS-25) runs; separate run: `GET /api/v1/server-info` at 1,000 req/s for 2 minutes

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| RSS increase from stalled connections | 100 MB | 200 MB |
| 1 GiB upload delay | 5% | 10% |
| `server-info` latency p95 | 2 ms | 10 ms |

**Resource limits**:
- Connection cap 4096 (production). `C-REF` for the delay bound; RSS bound also valid on `C`.

## Security Tests

### ST-11-1: CSRF and cookie replay
**Summary**: cross-site requests with a valid cookie cannot mutate.
**Traces to**: AC-58
**Attack vector**: attacker page posting to the API with the victim's cookie (form POST, fetch with `no-cors`, sibling subdomain).
**Test procedure**: IT-11-6 against Mode R for the real operations with a real session cookie and mock-idp; check that no state changed (ctl read-only probe).
**Expected behavior**: 403 `csrf_rejected` on every state-changing operation; same-origin requests still work.
**Pass criteria**: N of N operations rejected; 0 state changes.
**Fail criteria**: one mutation succeeds.

### ST-11-2: Error and header injection
**Summary**: errors leak nothing and header values cannot be injected.
**Traces to**: AC-47
**Attack vector**: CRLF in file names and query strings, `Content-Disposition` forging, request smuggling (CL + TE), duplicate `Host`, oversized method names.
**Test procedure**: Mode S/R requests with the listed inputs; read raw responses.
**Expected behavior**: smuggling variants rejected 400 or connection closed; response header values contain no raw CR/LF; problem bodies contain no SQL, path or stack text.
**Pass criteria**: 0 injected headers; 0 leaked internals.
**Fail criteria**: any.

### ST-11-3: Cross-account matrix completeness
**Summary**: the matrix generator covers every operation and ID parameter.
**Traces to**: AC-59
**Attack vector**: IDOR with another account's ids.
**Test procedure**: generate the two-account matrix from `openapi()` (all `x-id-params`); run against Mode R; then register an extra stub route without a matrix entry and rerun the generator in CI mode.
**Expected behavior**: 0 successful cross-account calls (all 404); the unmatched route makes the generator fail.
**Pass criteria**: matrix size equals the number of (operation, id parameter) pairs in the registry.
**Fail criteria**: any cross-account success or silent skip.

### ST-11-4: Worker listener exposure
**Summary**: worker resources are unreachable from the public path and from user credentials.
**Traces to**: AC-79
**Attack vector**: reaching `/api/v1/worker-jobs/**` through the proxy, or with user tokens on `:8081`.
**Test procedure**: IT-11-9 expectations with a probe set of 30 path variants (encoded slashes, `..`, case changes, trailing slash, `//api/v1/worker-jobs/status`).
**Expected behavior**: public path always 404; `:8081` without worker bearer always 403.
**Pass criteria**: 0 responses other than 404/403 respectively.
**Fail criteria**: any 2xx or distinguishable response.

## Acceptance Tests

### AT-11-1: Surface completeness
**Summary**: no undocumented routes and CSRF N of N.
**Traces to**: AC-58, AC-64
**Preconditions**:
- Mode R stack; release OpenAPI of the previous tag available.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Compare router and `sunnycloud openapi` output | 0 undocumented routes |
| 2 | Run CSRF matrix | 403 on N of N state-changing operations |
| 3 | Old client header on a protected route | 426 with `min_version` |

### AT-11-2: SIGTERM during 100 uploads
**Summary**: no acked block is lost.
**Traces to**: AC-38
**Preconditions**:
- Mode R stack; DS-34 files, 100 concurrent uploads; `ctl` sends SIGTERM to the api.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | SIGTERM after about 50% of bytes | `/readyz` 503, long-polls woken, process exits within `SHUTDOWN_GRACE` |
| 2 | Restart, clients retry | every file completes; hashes equal DS-34 manifest |
| 3 | Count blocks acked before SIGTERM | all present on disk; 0 zero-filled |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| stub scripts | per-route scripted port results (error kinds, panics, stream errors) | written with the tests | < 1 MB |
| hostile types | DS-17 hostile and positive-control files | `gen_hostile.py` | < 5 MB |
| OpenAPI fixtures | previous-release document, mutated copies | CI artifact | < 2 MB |
| DS-25, DS-30, DS-34 | 1 GiB file, stalled connections, 100 multiblock files | generators | 1 GiB, n/a, 2.5 GiB |

**Setup procedure**:
1. Mode S: build `HttpServer` with stub `ApiServices` in a Rust integration test; listen on ephemeral ports; the harness picks the h2c/h1 client.
2. Mode R: `tests/run.sh --profile core` with the test image (and the release image for IT-11-9); seed accounts through invites.

**Teardown procedure**:
1. Stop the server (the shutdown path is itself under test in IT-11-16); reset the clock offset; ctl account purge for Mode R.

**Data isolation strategy**: Mode S tests each own an ephemeral port and a fresh server; no shared state. Mode R tests create their own accounts (group P) or run on a restored golden stack (group X for kill, shutdown, DB outage). 11 has no queries, so no database stand-in is used anywhere; Mode R tests run on real PostgreSQL.

## Spec gaps found

- G-11-1: `need_blocks` and `edited_since` map to "409/412 per code"; the assignment is not given.
- G-11-2: behaviour for an absent or malformed `X-Sunnycloud-Client` header on non-exempt routes is not specified (BLOCKED part of IT-11-7).
- G-11-3: isolation in the other direction (public API routes on `:8081`) is not specified.
- G-11-4: the HTTP status of `/readyz` when `status: degraded` is not stated.
- G-11-5: no code is assigned to `LastAdmin` in the error table (07 cross-reference).
