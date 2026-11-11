# Contract: http_server — `HttpServer`, `ApiServices`, `HttpConfig`, readiness list, run and shutdown

**Component**: 11_http_api (library `sunnycloud-http-api`, proposed)
**Producer task**: E-21/c03 — `_docs/02_tasks/todo/B07-03_crate_skeleton_accept_loop.md` (contributors: E-21/c15 internal router and test-clock listener, E-21/c16 readiness list and ops listener, E-21/c17 shutdown semantics of `run`); keys assigned at Step 4
**Consumer tasks**: E-22/c03 (composition root builds `ApiServices`), E-22/c04 (binds `HttpConfig`), E-22/c05 (`serve` calls `run` and owns the rest of the shutdown order), E-22/c15 (`openapi` and `healthcheck` commands); E-31 harness (Mode R); resolved at Step 4
**Version**: 1.4.0
**Status**: draft
**Last Updated**: 2026-11-11

## Purpose

The library surface that the composition root (12) uses to run the API: one concrete `HttpServer` built from the business ports and a typed configuration, with four listeners and a graceful stop. 11 is a library; it builds no service and binds no configuration itself (architecture "Dependency rule", ADR-015).

Sources: `components/11_http_api/description.md` §2, §4, §10, §11; `epics/E-21_http_api.md` § Interface specification; `common-helpers/01_helper_config.md` (General and HTTP rows); `components/12_operator_cli/description.md` §5.

## Shape

### Functions

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-------------------------|--------|-----------|
| `HttpServer::new` | `(services: ApiServices, config: HttpConfig) -> Result<HttpServer, HttpServerError>` | `InvalidConfig` (a value the server cannot use, such as an unparsable trusted-proxy range) | sync, no I/O |
| `HttpServer::run` | `(self, shutdown_signal: future) -> Result<(), HttpServerError>`; binds every listener, serves until the signal, then runs the connection shutdown below and returns | `Bind{listener, addr}` before serving; nothing after serving starts | async |
| `HttpServer::openapi` | `(&self) -> OpenApiDocument` (the same bytes `GET /api/v1/openapi.json` serves) | none | sync |

Error names are proposals of E-21/c03; they become final when that task is accepted.

### `ApiServices` (one `Arc<dyn Port>` each; port traits from `contracts` parts 1-3)

| Field group | Ports |
|-------------|-------|
| 01 identity | `Authenticator`, `PrincipalRevalidator`, `SignInService`, `SessionService`, `DeviceGrantService`, `InviteService` |
| 02 tree | `TreeService`, `TreeReader` |
| 03 transfer | `UploadService`, `CommitService`, `DownloadService` |
| 05 sync_feed | `ChangeFeed` |
| 06 sharing | `ShareService`, `PublicShareService` |
| 07 admin | `AdminService`, `AccountService`, `AccountDeletionService` |
| 08 media_jobs | `ThumbnailStateSource`, `DerivedOpener`, `WorkerAuthenticator`, `WorkerJobs` |
| 20 vault_service | `VaultService`, optional, absent in v1.0 and v1.1 builds |
| Runtime handles | `ReadinessCheck` list (registered by 12), `feed_shutdown` (a `CancellationToken`, the same one 12 gives `PgChangeFeed::new`; `run` cancels it, see the shutdown order), the metrics registry of `sc_observability`, the `Clock` of `sc_clock` (the `OffsetClock` handle in builds with feature `test-clock`) |

### `HttpConfig` (typed section bound by 12 through `sc_config`; keys from helper 01)

| Field | Key (env `SUNNYCLOUD_<KEY>`) | Default |
|-------|------------------------------|---------|
| public origin | `public_url` | required; must be `http(s)://host[:port]` with at most one trailing slash (`HttpServer::new` returns `InvalidConfig` otherwise); the only source of the origin the CSRF check compares |
| trusted proxies | `trusted_proxies` | empty (default deny) |
| internal listener address | `internal_addr` | `0.0.0.0:8081` |
| ops listener address | `ops_addr` | `0.0.0.0:9090` |
| test-clock listener address (feature `test-clock` only; the field does not exist in release builds) | not in the key table (Open point 4) | `0.0.0.0:9091` |
| public listener address | not in the key table (Open point 1) | `:8080` |
| connection cap | `max_connections` | 4096 |
| HTTP/1 header timeout | `h1_header_timeout` | 10s |
| HTTP/2 stream / connection window | `h2_stream_window` / `h2_conn_window` | 4MiB / 32MiB |
| HTTP/2 streams, pending resets, header list | `h2_max_streams`, `h2_max_pending_reset`, `h2_max_header_list` | 100, 20, 16KiB |
| HTTP/2 keepalive interval / timeout | `h2_keepalive_interval` / `h2_keepalive_timeout` | 30s / 20s |
| request timeout, body idle timeout | `request_timeout`, `body_idle_timeout` | 60s, 30s |
| download write idle | `download_write_idle` | 60s |
| worker output body caps | `derived_max_thumb`, `derived_max_preview`, `derived_max_transcode` (owned by 08; `WorkerOutputLimits`, bytes) | 5MiB, 20MiB, 10GiB |
| shutdown grace | `shutdown_grace` | 25s |
| minimum client version | `min_client_version` | set by the release; one semver or `family=semver,...` |
| `ServerInfo` value | built by 12 from the typed sections and the build (`server_info.md`) | - |

### Listeners

| Listener | Serves | Notes |
|----------|--------|-------|
| public `:8080` | `/api/v1/**`, SPA bundle and SPA fallback | behind Caddy only, h2c prior knowledge and HTTP/1.1 on one port |
| internal `internal_addr` | `/api/v1/worker-jobs/**`; `GET /internal/v1/test/clock-offset` with feature `test-clock`; every other path 404 | worker bearer; the public router never contains these routes (AC-79) |
| ops `ops_addr` | `/healthz`, `/readyz`, `/metrics` (router from `sc_observability`) | never routed by Caddy |
| test-clock `:9091` | `POST /test/clock` | compiled only with feature `test-clock`; absent from release builds (FT-N-35); bound by `bind` with the other listeners (`BoundServer::test_clock_addr`, `Listener::TestClock` on a bind error); no credential (`api_v1_surface.md` open point 7) |

### `run` shutdown order (E-21/c17)

On the shutdown signal, in this order:

1. `/readyz` answers 503 (without running the checks).
2. Every listener stops accepting; a new connection is refused.
3. Held long-polls are woken: `run` cancels `ApiServices::feed_shutdown`, and 05's held reads return an empty page with the same cursor (`change-feed.md`). 12 must give 05 this token and no other part of 12 may wait on it, because it fires here and not at the process signal.
4. Open connections get a graceful close (HTTP/1.1 closes after the request in flight, HTTP/2 sends GOAWAY and lets open streams finish), so a response written on the wake of step 3 is sent before its connection closes; `run` waits for them for at most `shutdown_grace`.
5. At the end of the grace, every connection still open is dropped. A response still streaming ends as a connection close (HTTP/1.1: before its `Content-Length`; HTTP/2: the stream and connection are closed, the client sees a broken pipe or reset), never as a clean short body.
6. `run` returns when no connection is open: at once when they all finished, otherwise at the end of the grace, so within grace + 1 s of the signal.

The clock of the grace starts at step 4. What 12 does after `run` returns (scheduler stop, upload checkpoints, final group sync, pool close) is 12's.

## Invariants

- 11 constructs no business service, no scheduler, no rate limiter and no database pool; it only holds what `ApiServices` gives it.
- Every handler calls exactly one port of `ApiServices` (the share thumbnail route is the documented composition of 06, 08 and 03, see `api_v1_surface.md`).
- Auth is checked before any request body is read; an unread body is dropped and its flow-control credit released.
- Excess connections above `max_connections` are refused and counted in `connections_rejected_total{reason}`.
- Builds without feature `test-clock` contain neither test-clock route nor the `:9091` listener.
- `openapi()` is deterministic: two calls on one build return identical bytes.

## Non-Goals

- Not covered: config loading and validation (`sc_config`, E-22/c04); the ops router itself and the `/readyz` body (`sc_observability`); the process start and stop sequence outside the HTTP layer (E-22/c05); TLS (ends at Caddy).
- Not covered: the route list and wire rules (`api_v1_surface.md`), the error body (`problem_details.md`), `ServerInfo` (`server_info.md`).

## Versioning Rules

- **Breaking**: a field of `ApiServices` or `HttpConfig` removed or retyped, a changed `run` shutdown order, a port removed. Needs a new major version and notice to E-22.
- **Non-breaking**: a new optional port (for example `VaultService` in v1.2), a new config field with a default.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| build-and-run | `ApiServices` of port fakes, ephemeral ports | all listeners answer; `run` returns after the signal | Mode S harness of E-21/c03 |
| h2-settings | raw h2c client reads SETTINGS | initial window 4194304, connection window 33554432 after the first WINDOW_UPDATE, max streams 100, max header list 16384 | IT-11-18 |
| connection-cap | 60 connections, `max_connections = 50` | 10 refused, `connections_rejected_total` +10 | IT-11-19 |
| shutdown-order | 20 held requests, 3 downloads, grace 2 s | readiness 503 < accept stops < held requests answered < connections closed; long download cut at grace; `run` returns within grace + 1 s | IT-11-16; until the changes route exists the held requests are worker `claim` calls waiting on `feed_shutdown` |
| release-build | build without `test-clock` | `:9091` refused, `/internal/v1/test/clock-offset` 404 | IT-11-9, FT-N-35; the default-feature tests assert the 404s and the empty route table, the refusal follows from the listener and `test_clock_addr` not compiling |

## Open points

1. The public listener address (`:8080`) has no key in helper 01's table; `containerization.md` also says the ops address key "is still to be added" although helper 01 lists `ops_addr`. E-22/c04 and E-01 owner to add the public key.
2. `ReadinessCheck` registry: E-21's interface lists a registry provided by 11, while `sc_observability` provides `ReadinessRegistry` and the ops router. This contract follows `sc_observability`: 11 receives the list and mounts the shared router; it adds no second registry.
3. `AbuseLimiter`: 12 "injects it into 11 where it needs it"; no 11 behaviour in v1.0 needs it (11 only maps `RateLimited`), so it is not an `ApiServices` field.
4. The test-clock listener address has no key in helper 01's table. `HttpConfig.test_clock_addr` is a field of test builds only; E-22/c04 and the E-01 owner to add the key (default `0.0.0.0:9091`, as the table above) and to set the field only under feature `test-clock`.

5. The ops listener stops accepting at step 2 like the others, so after the signal `/readyz` answers 503 only on a connection that is still busy, and a probe on a new connection is refused. If operators want `/readyz` and `/metrics` scrapeable through the grace, the ops listener would have to stop last; the owner decides, nothing needs it today.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B07) |
| 1.1.0 | 2026-11-08 | `HttpConfig.worker_output_limits` added; `request_timeout` and `body_idle_timeout` must be above zero; `BodyLimits` is a layer of the route stack | AZ-3803 |
| 1.1.1 | 2026-11-08 | `HttpConfig.download_write_idle` (must be above zero); the accept loop wraps every socket in a write-idle guard | AZ-3803 |
| 1.2.0 | 2026-11-08 | `HttpConfig.public_url` validated as an origin; `CsrfGuard` is layer 9 of the route stack (`RouteContext.csrf`) | AZ-3808 |
| 1.3.0 | 2026-11-11 | Worker routes registered for the internal listener; with feature `test-clock`: `HttpConfig.test_clock_addr`, the `:9091` listener bound by `bind` and served by `serve`, `BoundServer::test_clock_addr`, `Listener::TestClock`; Open point 4 | AZ-3831 |
| 1.4.0 | 2026-11-11 | `run` shutdown order made exact: `ApiServices.feed_shutdown` added and cancelled by `run` after accept stops; connections get a graceful close and are dropped at the end of `shutdown_grace`; `run` returns when none is open | AZ-3833 |
