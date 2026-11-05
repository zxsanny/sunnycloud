# HTTP API crate skeleton, `HttpServer`, `ApiServices`, `HttpConfig` and the accept loop

**Task**: AZ-3801_crate_skeleton_accept_loop
**Name**: HTTP API skeleton and accept loop
**Description**: The 11 library with its public surface, the hand-built accept loop shared by all listeners (connection cap, HTTP/1.1 and h2c on one port, HTTP/2 tuning, keepalive, header timeout) and the stub-services test harness every later E-21 task uses
**Complexity**: 3 points
**Dependencies**: AZ-3768_contracts_part1, AZ-3769_contracts_part2, AZ-3770_contracts_part3, AZ-3763_sc_config, AZ-3765_sc_clock, AZ-3786_sc_observability, AZ-3652_initial_structure
**Component**: 11 http_api
**Tracker**: AZ-3801
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md`, `part2_tree_transfer_feed.md`, `part3_blob_share_media_vault.md` — the port traits held in `ApiServices` (E-00/c03..c05).
- `_docs/02_document/contracts/shared_config/sc_config.md` — typed sections and duration settings (E-01/c01).
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `Clock` and `OffsetClock` (E-03/c01).
- `_docs/02_document/contracts/shared_observability/sc_observability.md` — metrics registry (E-09/c01).

## Problem

The composition root (12) needs one concrete server to build from its ports and config, and the API must keep 1 Gbps reachable on the Caddy-to-API hop (AC-2) while bounding what an attacker can hold open (AC-59, AC-1). The default HTTP/2 window (64 KiB) caps a stream near 26 Mbit/s, and the framework's default serve function does not expose the HTTP/2 limits, so the accept loop is built by hand. Every later E-21 task also needs a way to run the server against scripted ports over real TCP.

## Outcome

- `HttpServer::new(ApiServices, HttpConfig)`, `run(shutdown_signal)` and `openapi()` exist as the contract states; the crate builds with the `test-clock` feature on and off.
- Every listener runs the same accept loop with the connection cap, HTTP/1.1 and h2c prior knowledge on one port, and the configured HTTP/2 and keepalive settings.
- A stub-services harness (Mode S of `components/11_http_api/tests.md`): an `ApiServices` whose ports are scripted fakes (unscripted calls fail as unexpected), a server on ephemeral ports, and real HTTP/1.1 and h2c clients over TCP.

## Scope

### Included
- The library's public surface (`HttpServer`, `ApiServices`, `HttpConfig`, `ServerInfo` type placeholder for E-21/c06) per `http_server.md`.
- The accept loop: one task per connection, a global connection semaphore (`max_connections`; excess refused and counted in `connections_rejected_total{reason}`, `connections_open` gauge), HTTP/1.1 and h2c on one port, HTTP/2 stream and connection windows, max concurrent streams, max pending resets, max header list, keepalive interval and timeout, HTTP/1 header timeout.
- Binding the public and internal listeners from `HttpConfig`; the ops and test-clock listeners are bound by E-21/c16 and E-21/c15.
- The Mode S harness and its use in this task's tests.

### Excluded
- Middleware, routes, error mapping (E-21/c04..c11); shutdown order beyond stopping the accept loop (E-21/c17).
- Config binding and validation (E-22/c04, `sc_config`); constructing services (E-22/c03).

## Acceptance Criteria

**AC-1: Surface builds and runs**
Given an `ApiServices` of port fakes and an `HttpConfig` with ephemeral ports
When `HttpServer::new` is called and `run` is started, then the shutdown signal is sent
Then the public and internal listeners accept HTTP/1.1 and h2c connections on their ports, and `run` returns

**AC-2: HTTP/2 settings match the configuration (AC-2)**
Given default settings, and a second run with overridden values
When a raw h2c client reads the server's SETTINGS and WINDOW_UPDATE frames
Then the initial stream window is 4194304, the connection window is 33554432 after the first WINDOW_UPDATE, max concurrent streams is 100 and max header list is 16384 (or the overridden values); HTTP/1.1 is served on the same port

**AC-3: Keepalive closes dead peers**
Given a keepalive interval and timeout shortened for the test
When a peer stops answering pings
Then the server sends pings at the interval and closes the connection after the timeout

**AC-4: Connection cap (AC-1, AC-59)**
Given `max_connections = 50`
When 60 connections are opened
Then connections 51 to 60 are refused or closed, `connections_rejected_total` grows by 10, and the first 50 keep working

**AC-5: Abusive peers are cut off (AC-59)**
Given a header timeout of 1 s and `max_pending_reset = 20`
When an HTTP/1.1 client drips headers at 1 byte per 300 ms, a client sends 1,000 HEADERS and RST_STREAM pairs in 1 s on one h2 connection, a client floods CONTINUATION frames, and a client sends a 17 KiB header list
Then the slow client is closed within 2 s, the rapid reset ends with GOAWAY and close, and the flood and oversized header list are refused (4xx, 431 or stream reset, never 2xx)

**AC-6: Harness available**
Given the Mode S harness
When a test scripts one port result and calls a route over h2c and over HTTP/1.1
Then the call reaches the scripted fake through real TCP, and an unscripted port call fails as an unexpected error rather than a default success

## Non-Functional Requirements

**Performance**
- The accept loop adds no buffering of request bodies; windows are the configured values (PT-11-1 measures throughput in E-21/c20).

**Compatibility**
- tokio >= 1.54.1, h2 >= 0.4.16, bytes >= 1.11.1 (`cargo deny` floors, RESTRICT-12); axum 0.8.9, hyper 1.12, hyper-util as pinned by the epic.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Mapping from `HttpConfig` to the HTTP/2 builder settings | Every setting carried, none left at the library default |
| AC-4 | Connection semaphore under 60 acquisitions with 50 permits | 10 refusals counted |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Fakes, ephemeral ports | Start, request, stop | Both listeners serve h1 and h2c; `run` returns | |
| AC-2 | Defaults and overrides | Raw h2c SETTINGS read (IT-11-18) | Values as configured | AC-2, NFT-PERF-02 |
| AC-3 | Short keepalive | Silent peer (IT-11-18) | Pings sent, peer closed | |
| AC-4 | Cap 50 | 60 connections (IT-11-19) | 10 refused, metric +10 | AC-1, NFT-RES-21 |
| AC-5 | Timeout 1 s, reset cap 20 | Slowloris, rapid reset, CONTINUATION, 17 KiB headers (IT-11-19) | Closed or refused, never 2xx | NFT-SEC-15 |
| AC-6 | Harness | Scripted and unscripted port calls | Reaches fake; unscripted fails | |

## Constraints

- No business logic, SQL or component crate in this crate (crate-graph test of AZ-3652); 11 imports `contracts` and helpers only.
- Every duration is a config value; tests use short ones.

## Risks & Mitigation

**Risk 1: Large windows become buffered memory**
- *Risk*: 32 MiB per connection times many peers.
- *Mitigation*: auth before body (E-21/c07), connection and stream caps here, NFT-SEC-15 gate in E-21/c20; reduce windows first if it fails.

## Runtime Completeness

- Named capability: HTTP/2 flow-control and abuse limits on the Caddy-to-API hop.
- Production code that must exist: the hand-built accept loop applying every configured HTTP/2 setting, the connection semaphore and the header timeout.
- Unacceptable substitutes: the framework's default serve function with library-default windows; tests that check the config struct instead of the SETTINGS frames on the wire.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The public listener address (`:8080`) has no key in helper 01's table | E-01, E-22 owners | open | Low |
| E-21 lists a `ReadinessCheck` registry provided by 11, while `sc_observability` provides `ReadinessRegistry`; this task follows `sc_observability` (no second registry) | E-09, E-21 owners | open | Low |
| `HttpServerError` variant names (`InvalidConfig`, `Bind`) are proposals of this task | E-21 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/11_http_api/http_server.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
