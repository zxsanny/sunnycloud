# Client version gate and `ServerInfo` route

**Task**: AZ-3804_version_gate_server_info
**Name**: Version gate and ServerInfo
**Description**: Define `ServerInfo`, serve it at `GET /api/v1/server-info` to any client version, and refuse clients below the configured minimum with 426 `upgrade_required` before authentication, using the `X-Sunnycloud-Client` header
**Complexity**: 3 points
**Dependencies**: AZ-3810_error_mapper, AZ-3827_route_registry_openapi
**Component**: 11 http_api
**Tracker**: AZ-3804
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — the `Public` class and the system routes (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — the 426 and 400 bodies (E-21/c09).

## Problem

An old client talking to a newer server can corrupt data or loop on errors; AC-64 requires a clear upgrade message instead. Clients also need the protocol limits (block size, lanes, batch size, staged cap, long-poll wait) before they upload, and an old client must still be able to read the minimum version to show the upgrade prompt.

## Outcome

- `ServerInfo` with exactly the eight fields of `server_info.md`, built by 12 and served unauthenticated with `Cache-Control: no-store`.
- The gate as layer 7: below the minimum gives 426 with `min_version` and `update_url`; public routes and `server-info` are exempt; absent header accepted only with a browser-session cookie; malformed header 400.
- `sync_client_versions_total{client, supported}` counted by the gate.

## Scope

### Included
- The `ServerInfo` type and its OpenAPI schema; the `server-info` handler returning the value handed in by 12.
- Parsing `X-Sunnycloud-Client` (`web`, `android`, `ios`, `desktop` / semver) and the minimum (one semver or `family=semver,...`).
- The gate's position before auth extraction; its exemptions taken from the registry's `Public` class.
- The 426 body text containing the word "upgrade".

### Excluded
- Building the `ServerInfo` value from config and the build (E-22); config validation of `min_client_version` (`sc_config`).
- Client behaviour on 426 (E-23/c07, E-26).

## Acceptance Criteria

**AC-1: ServerInfo fields (AC-64)**
Given a `ServerInfo` built from `max_lanes=3`, `batch_max_entries=250`, `max_staged=7`, `longpoll_wait_max=20s` and minimum `1.2.0`
When `GET /api/v1/server-info` is called without credentials, and again with `X-Sunnycloud-Client: web/0.1.0`
Then both answer 200 with exactly the keys `block_size`, `max_lanes`, `small_file_max`, `batch_max_entries`, `max_staged_per_account`, `longpoll_max_wait`, `min_client_version`, `api_versions`, with `block_size = 2097152`, `small_file_max = 2097152`, `max_lanes = 3`, `batch_max_entries = 250`, `max_staged_per_account = 7`, `longpoll_max_wait` equal to 20 s, `min_client_version = "1.2.0"`, `api_versions = ["v1"]`, no provider list and no setup flag, `Cache-Control: no-store`, and a body valid against the OpenAPI schema

**AC-2: Old clients get 426 (AC-64, FT-N-27)**
Given minimum `1.2.0`
When a protected route is called with `android/1.1.9`, with `web/1.2.0` and with `desktop/1.3.0`
Then `android/1.1.9` gets 426 with `code: upgrade_required`, `min_version`, `update_url` and a `title` or `detail` containing "upgrade", and the stub port is not called; the other two pass the gate

**AC-3: Per-family minimum**
Given minimum `web=1.0.0,android=1.5.0`
When `web/1.0.0` and `android/1.4.9` call a protected route
Then `web/1.0.0` passes and `android/1.4.9` gets 426 with `min_version = 1.5.0`

**AC-4: Exemptions and order**
Given minimum `1.2.0`
When `ios/0.9.0` calls a public route and `server-info`, and an unauthenticated `android/1.1.9` calls a protected route
Then the first two pass, and the third gets 426 (not 401)

**AC-5: Absent and malformed headers (G-11-2)**
Given a protected route
When it is called without the header with a browser-session cookie, without the header with a bearer token, and with `foo` and with `web/x.y`
Then the cookie request passes the gate, the bearer request is refused (status recorded), and both malformed values get 400

**AC-6: Version counter**
Given 3 supported and 2 unsupported requests from `android`
When `/metrics` is read
Then `sync_client_versions_total{client="android", supported="yes"}` is 3 and `supported="no"` is 2

## Non-Functional Requirements

**Performance**
- `server-info` p95 2 ms, failure 10 ms at 1,000 req/s (PT-11-2, run in E-21/c20).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Minimum parsing and comparison (single, per family, semver edges) | Correct pass or refuse |
| AC-5 | Header parsing for valid, absent and malformed values | Valid parsed; malformed rejected |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, non-default config | ServerInfo (IT-11-8) | Exact keys and values | FT-P-05 |
| AC-2 | Mode S, minimum 1.2.0 | Version gate (IT-11-7) | 426 with upgrade text; port untouched | FT-N-27 |
| AC-3 | Mode S, per-family minimum | Version gate (IT-11-7) | Per-family result | |
| AC-4 | Mode S | Exemptions and order (IT-11-7) | Exempt pass; 426 before 401 | |
| AC-5 | Mode S | Absent and malformed (IT-11-7) | Cookie passes; malformed 400 | |
| AC-6 | Mode S | Counter | Counts per family and support | |

## Constraints

- The gate runs before auth and before any body is read; a refused request changes no state.

## Risks & Mitigation

**Risk 1: Raising the minimum strands users**
- *Risk*: an operator raises the minimum while many clients are old.
- *Mitigation*: AC-6's counter lets operators see old clients first (`observability.md`).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Status for an absent header on a bearer request is not specified (G-11-2 says only "must send it") | E-21 owner | open | Low |
| Source of `update_url` is not specified; helper 01 has no key | E-01, E-21 owners | open | Medium |
| Wire unit of `longpoll_max_wait` is not specified | E-21, E-23 owners | open | Low |
| Rule for a family missing from a `family=semver,...` list is not specified | E-21 owner | open | Low |
| `observability.md` lists providers and a version in `server-info`, contradicting 11 §8; this task follows 11 | E-21 owner, deployment | open | Low |
| Exact upgrade message text (G-19) | E-31/c03 | open | Low |
| `sync_client_versions_total` has no named producer in `observability.md`; counted here as the only reader of the header | E-09, E-21 owners | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/11_http_api/server_info.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
