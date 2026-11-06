# HttpSyncClient::connect, ServerInfo validation, version header, 426 and transport policy

**Task**: AZ-3959_http_sync_client_connect
**Name**: Client connect and transport policy
**Description**: Construct the one production `SyncApi` client: validate the server address, read and range-check `ServerInfo`, negotiate the API version, send the client header on every request, turn 426 into `UpgradeRequired`, and enforce TLS 1.3, HTTP/2 and HTTPS only
**Complexity**: 5 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3958_sync_typed_errors, AZ-3967_sync_host_traits_foundation, AZ-3970_scripted_fake_server
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3959
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — this task produces the trait, construction, negotiation and transport parts.
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CredentialProvider` (E-23/c14).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `ServerInfo`, `ServerUrl` (E-23/c04).
- `_docs/02_document/contracts/11_http_api/server_info.md` — `GET /api/v1/server-info`, header grammar, 426 body (draft, E-21/c06).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — routes and cross-route rules (draft, E-21/c11).

## Problem

A client must refuse an insecure server before any byte is sent (AC-54), refuse a server whose protocol constants differ (block size is fixed at 2 MiB), and show a clear upgrade message when it is too old instead of behaving wrongly (AC-64, FT-N-27). Every number the server sends must be range-checked, because a malicious server could otherwise force huge allocations (13 §7). The transport choices (TLS 1.3, ALPN h2, explicit windows, keep-alive for held long-polls) decide AC-2. Nothing constructs a client yet; every operation task (E-23/c08..c13) needs one.

## Outcome

- The `SyncApi` trait and `HttpSyncClient::connect(SyncConfig, Arc<dyn CredentialProvider>) -> (HttpSyncClient, ServerInfo)` with `ConnectError` as in the contract.
- Negotiated limits (lanes `min(4, max_lanes)`, batch size, staged cap, long-poll wait) available to every operation; server limits win over local settings.
- A transport that only speaks TLS 1.3 with ALPN h2 to the configured host, with the configured windows and keep-alive, and a release build without the plain-HTTP feature, checked in CI.

## Scope

### Included
- `connect`: `ServerUrl` check, `GET /api/v1/server-info`, block-size check, `api_versions` and `min_client_version` checks, range checks of every number, lane clamp.
- `X-Sunnycloud-Client: <family>/<semver>` and `Accept` on every request except `server-info`; 426 on any request mapped through the wire table to `UpgradeRequired` with the server text and never retried.
- Transport: one HTTP/2 client per `HttpSyncClient`, TLS 1.3 minimum, ALPN h2 required, HTTPS only, windows `sync.h2_stream_window`/`sync.h2_conn_window` with adaptive window off, keep-alive pings every `sync.keepalive_interval` while idle, `sync.connect_timeout` and `sync.request_idle_timeout`, the system trust store, no redirect to another host.
- The compile-time feature `allow-plain-http` (debug builds only) and the CI check that release binaries lack it.

### Excluded
- Operations (E-23/c08..c13), the request plumbing for credentials, cancellation and rate limits (E-23/c14), the Android verifier initialisation entry point (E-23/c15).
- Tuning the window values and a second connection (E-23/c02).

## Acceptance Criteria

**AC-1: Plain HTTP refused before any socket (AC-54)**
Given `sync.server_url = http://sunnycloud.test` in a release-profile build
When `connect` runs
Then it returns `InsecureScheme` and no socket is opened

**AC-2: ServerInfo negotiation (AC-64)**
Given fake servers reporting `block_size` 1 MiB; `min_client_version` above the client; `api_versions` without `v1`; `max_lanes` 2; `max_lanes` 16
When `connect` runs against each
Then the results are `ProtocolMismatch{what: block_size}`; `UpgradeRequired{message, update_url}`; `UpgradeRequired`; lanes 2; lanes 4

**AC-3: Hostile limits are refused or clamped (13 §7)**
Given `ServerInfo` with `small_file_max = 2^63`, `batch_max_entries = 0`, `max_lanes = 0` and a negative `longpoll_max_wait`
When `connect` runs against each
Then each ends within 2 s in a typed error or a documented clamp, with no panic and allocation at most 64 MiB

**AC-4: Version header and 426 (AC-64, FT-N-27)**
Given a connected client and a fake server that logs headers and later answers one request with 426 `upgrade_required`
When the client sends a series of requests
Then every request except `GET /api/v1/server-info` carries `X-Sunnycloud-Client: <family>/<semver>`, and the 426 yields `UpgradeRequired` with the server text and 0 retries

**AC-5: TLS 1.3 and HTTP/2 only (AC-54)**
Given a TLS 1.2-only fake server and a TLS 1.3 fake server without h2
When `connect` runs against each
Then both fail with a typed connection error and no request body is sent

**AC-6: No cross-host redirect (13 §7)**
Given a fake server answering a request with `302` to `https://evil.test/`
When the client sends that request
Then the redirect is not followed and `evil.test` receives 0 requests and 0 `Authorization` headers

**AC-7: Release builds lack plain HTTP**
Given the release build of the client crate and of every binary that links it
When the CI feature check runs
Then `allow-plain-http` is absent, and a seeded release build with the feature fails the check

## Non-Functional Requirements

**Performance**
- Windows 8 MiB stream / 32 MiB connection by default (above the 2.5 MB bandwidth-delay product at 1 Gbps x 20 ms); values tuned only by E-23/c02.

**Security**
- No request goes to a host other than `sync.server_url`; certificates are verified against the system trust store.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Negotiation decisions for each `ServerInfo` case | As stated (IT-13-13) |
| AC-3 | Range checks and clamps | Typed error or clamp, no panic (ST-13-3 limits part) |
| AC-7 | Feature check on a seeded build | Fails on the seeded build |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Release-profile build, packet capture | `connect("http://...")` | `InsecureScheme`, 0 packets | IT-13-13, ST-13-1, FT-N-26, NFT-SEC-12 |
| AC-2 | Scripted fake server (E-23/c17) | Each `ServerInfo` case | Typed result per case | IT-13-13, FT-N-27 |
| AC-3 | Scripted fake server | Hostile limits | Error or clamp in 2 s, <= 64 MiB | ST-13-3 |
| AC-4 | Scripted fake server with header log | Header on every request; 426 | Header present; `UpgradeRequired`, 0 retries | IT-13-13, AT-13-2 step 3 |
| AC-5 | TLS 1.2-only and h2-less fakes | Handshake | Failure, no body sent | ST-13-1 |
| AC-6 | Fake 302 to `evil.test` | Redirect handling | Not followed, 0 credentials at `evil.test` | ST-13-3 |

## Runtime Completeness

- **Named capability**: the production HTTP/2 client over rustls (reqwest 0.13.5, h2 >= 0.4.16, rustls >= 0.23.45, rustls-platform-verifier 0.7.1) with TLS 1.3, ALPN h2, explicit windows and keep-alive.
- **Production code that must exist**: real TLS connections verified against the system trust store, `server-info` read over the network, the version header on every real request.
- **Allowed external stubs**: the scripted fake server (E-23/c17) as the peer in tests; the server is an external process.
- **Unacceptable substitutes**: a plain-HTTP or certificate-skipping path reachable in release builds, a canned `ServerInfo` inside the crate, an HTTP/1.1 fallback.

## Constraints

- One HTTP/2 connection carries all lanes until E-23/c02 decides otherwise (13 risk 1).
- The client imports no server crate and no `contracts`; the OpenAPI document of 11 is the authority for wire shapes.

## Risks & Mitigation

**Risk 1: Window values miss AC-2**
- *Risk*: the start values are not enough through Caddy.
- *Mitigation*: values are config; E-23/c02 measures and sets them before PT-13-1.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Gate S1 may require a second HTTP/2 connection (13 risk 1, `sync_api.md` open point 6); this task builds one connection | E-23/c02 | open | Low |
| `ConnectError::Unauthorized` is listed by 13 but `connect` only calls the public `server-info`; when it occurs is not stated | E-23 | open | Low |
| The `update_url` source and the exact upgrade text are open (`server_info.md` open points 2, G-19); the client shows whatever the server sends | E-21 owner | open | Low |

## Contract

This task produces the trait, construction, negotiation and transport parts of the contract at `_docs/02_document/contracts/13_sync_core/sync_api.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
