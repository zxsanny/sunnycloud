# Contract: server_info — `GET /api/v1/server-info`, `X-Sunnycloud-Client` and the version gate

**Component**: 11_http_api
**Producer task**: E-21/c06 — `_docs/02_tasks/todo/B07-06_version_gate_server_info.md`; keys assigned at Step 4
**Consumer tasks**: E-22/c03 or E-22/c04 (builds the `ServerInfo` value from the typed config and the build), E-23/c04 (`sync-core-proto` DTO), E-23/c07 (`HttpSyncClient::connect`, version header, 426), E-26/c06 (SPA client, `web/<version>` header), E-24 daemon and E-27 Android `set_server` probes (through `sync-core`/`mobile_backup`), E-31 (FT-N-27, FT-P-05); resolved at Step 4
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

What a client reads before anything else (protocol limits and the minimum supported client version) and how the server refuses a client that is too old with a clear upgrade answer instead of corrupt behaviour (AC-64). `ServerInfo` is defined by 11 and built by 12.

Sources: `components/11_http_api/description.md` §5 layer 7, §8 (`ServerInfo`); `canonical-names.md` § Runtime; `components/13_sync_core/description.md` §2, §5; `tests/spec-gaps.md` G-11-2, G-19.

## Shape

### `GET /api/v1/server-info` (Public, exempt from the version gate, `Cache-Control: no-store`)

| Field | Type | Required | Description | Constraints |
|-------|------|----------|-------------|-------------|
| `block_size` | integer (bytes) | yes | protocol constant | 2097152 |
| `max_lanes` | integer | yes | config key `max_lanes` | |
| `small_file_max` | integer (bytes) | yes | staged-upload cap, protocol constant | 2097152 |
| `batch_max_entries` | integer | yes | config key `batch_max_entries` | default 500 |
| `max_staged_per_account` | integer | yes | config key `max_staged` | |
| `longpoll_max_wait` | duration | yes | config key `longpoll_wait_max` | unit open (Open point 1) |
| `min_client_version` | string | yes | configured minimum (one semver, or per family) | |
| `api_versions` | array of strings | yes | served `/api/vN` families | `["v1"]` in v1.0 |

No provider list and no setup flag: clients read those from `GET /api/v1/auth/providers` (01). Clients ignore unknown fields.

### Request header `X-Sunnycloud-Client: <family>/<semver>`

- `family` is one of `web`, `android`, `ios`, `desktop`; `semver` is `MAJOR.MINOR.PATCH`.
- Native clients send it on every request; the bundled web app sends `web/<version>`.

### Gate (runs before authentication)

| Request | Answer |
|---------|--------|
| Public route or `GET /api/v1/server-info` | passes, whatever the header |
| Header version below the minimum for its family | 426 `upgrade_required` with `min_version` and `update_url` |
| Header version equal or higher | passes |
| Header absent, browser-session cookie present | passes |
| Header absent, no cookie (native or bearer request) | refused (status open, Open point 3) |
| Malformed header (`foo`, `web/x.y`) | 400 `invalid_request` |

Configuration `min_client_version` is either one semver for all families or `family=semver,...`; the rule for a family not named in the list is open (Open point 4).

### 426 body

`application/problem+json` (`problem_details.md`) with `code: upgrade_required`, `min_version` (the minimum for the caller's family), `update_url`, and a `title` or `detail` text that contains the word "upgrade" (FT-N-27; exact text open, G-19).

## Invariants

- `server-info` reveals nothing beyond what the sign-in page shows and is answerable by any client version, so an old client can read `min_client_version` and show the upgrade prompt.
- The gate refuses before any handler runs: a refused request changes no state (FT-N-27 step 2).
- `block_size` and `small_file_max` never change within `/api/v1`; a client that sees another `block_size` refuses to upload (16, 13).

## Non-Goals

- Not covered: building the value (12) and the config keys' validation (`sc_config`); the updater manifest (S4-3).
- Not covered: the provider list and setup flag (01 routes).

## Versioning Rules

- **Breaking**: a field removed, renamed or retyped; a change of `block_size` inside `/api/v1`; a stricter header grammar.
- **Non-breaking**: a new optional field, a new `api_versions` entry.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| fields | config `max_lanes=3`, `batch_max_entries=250`, `max_staged=7`, `longpoll_wait_max=20s`, minimum 1.2.0 | exactly the 8 keys; values as configured; `api_versions = ["v1"]` | IT-11-8 |
| old-client | `android/1.1.9`, minimum 1.2.0 | 426 with `min_version`, `update_url`; no handler ran | IT-11-7 |
| per-family | `web=1.0.0,android=1.5.0` | each family checked against its own minimum | IT-11-7 |
| exempt | `ios/0.9.0` on `server-info` and on a public route | 200 | IT-11-7, IT-11-8 |
| before-auth | unauthenticated old client on a protected route | 426, not 401 | IT-11-7 |
| malformed | `foo`, `web/x.y` | 400 | G-11-2 |

## Open points

1. The unit of `longpoll_max_wait` on the wire (seconds or milliseconds) is not specified.
2. The source of `update_url` is not specified; helper 01 has no key for it.
3. G-11-2 settles "native clients must send the header" but not the status for a missing header without a cookie (400 like a malformed value is the closest rule).
4. A family missing from a `family=semver,...` list: no minimum or a default minimum is not specified.
5. `deployment/observability.md` lists `server-info` with "version ... plus providers", which contradicts 11 §8 (no providers). This contract follows 11.
6. `sync_client_versions_total{client, supported}` (`observability.md`) has no named producer; the gate is the only reader of the header, so E-21/c06 proposes to count it.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B07) |
