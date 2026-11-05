# Host traits, MemoryCheckpointStore, cancel(), Secret redaction, rate-limit handling

**Task**: AZ-3967_sync_host_traits_foundation
**Name**: Client foundation: host traits and request plumbing
**Description**: Declare the five host traits with `MemoryCheckpointStore`, bind the typed `sync` config section, and build the shared request path every operation uses: per-request credentials kept secret, cooperative cancellation, `Retry-After` and `AccountNotActive` handling, and structured logs without tokens or names
**Complexity**: 5 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3958_sync_typed_errors, AZ-3970_scripted_fake_server, AZ-3763_sc_config, AZ-3765_sc_clock
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3967
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — produced by this task.
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `SyncConfig`, `cancel()`, common errors (this task contributes them).
- `_docs/02_document/contracts/sync-core/crate_skeleton.md` — the six client traits live in `sync-core` (E-00/c07).
- `_docs/02_document/contracts/shared_config/sc_config.md` — key table, `Secret<T>`, client reload (E-01/c01).
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — clock for retry timing, `ManualClock` (E-03/c01).

## Problem

Every operation of E-23 needs the same pieces: the host traits it calls, a credential that is read per request and never printed, a way for the host to stop everything (UniFFI has no async cancellation, so the mobile host calls `cancel()` from `onStopped`), one rule for 429 `Retry-After`, and a stop-all when the account is disabled. If each operation task built its own, tokens would leak on one path and cancellation would miss another. The E-00 skeleton left the trait declarations to E-23 because their types live in `sync-core-proto` (`crate_skeleton.md`). This row is ordered late in the epic table but every executor task depends on it; its `cancel()` and rate-limit behaviour across finished operations is re-checked in the real-server suite (E-23/c18a, IT-13-18). Points raised from 3 to 5 because the typed `sync` config section (13 §5) has no other home in the epic.

## Outcome

- `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore` (with batch records), `ProgressSink`, `Credential` and `MemoryCheckpointStore` as in the `host_traits` contract, declared once in `sync-core`.
- The typed `sync` section with its key rows added to helper 01's table (`SUNNYCLOUD_SYNC_*`), validated, reloadable through `reload_client_settings`.
- One request path used by every operation: `bearer()` per request, credentials as `Secret`, cancellation, `Retry-After`, `AccountNotActive` stop, structured logs.

## Scope

### Included
- The five traits and `Credential` (object-safe, `Send + Sync`), `on_rotated` returning a persist error (Step 5 gap default), `MemoryCheckpointStore`.
- `Secret` reused from `sc_config`; no second redaction type.
- `SyncConfig` keys and defaults of 13 §5 (durations as config values; server limits win at connect).
- Request path: credential per request, never cached longer than one request; tokens only in headers or bodies, never in URLs or error text.
- `cancel()`: one cancel signal per client; every in-flight request ends with `Cancelled` within 1 s; nothing is sent afterwards.
- 429 with `Retry-After`: the next attempt not before the given delay, jitter under 1 s; `rate_limited` surfaces as `RateLimited{retry_after}` when retries are not allowed.
- `AccountNotActive` (403): the client stops all work; no further requests until a new client is built.
- Logs: ERROR protocol violation and unexpected status, WARN retry with attempt and delay, resync required, lost-commit recovery, INFO connect and upload finished; fields `op`, `attempt`, `status`, `request_id`; no names, paths or tokens; no per-block logging.

### Excluded
- `connect` and transport (E-23/c07), operations (E-23/c08..c13), refresh on 401 (E-23/c13).
- Host implementations of the traits (14, 17, 18).

## Acceptance Criteria

**AC-1: Traits declared once, in the client crate**
Given the workspace
When the crate graph and a name search run
Then the five traits and `Credential` are declared only in `sync-core`, nothing in `contracts` names them, and `CheckpointStore` has no cursor method

**AC-2: MemoryCheckpointStore is consistent**
Given 8 threads saving, loading and clearing checkpoints and batch records concurrently
When they finish
Then every load returns the last saved value for its key and `load_batches` returns exactly the batches saved and not cleared

**AC-3: Config section binds and validates**
Given the defaults, a config file and `SUNNYCLOUD_SYNC_RETRY_BASE=2s`
When the `sync` section is loaded
Then `retry_base` is 2 s, every other key has its 13 §5 default, an out-of-range value is reported with its key, and an unknown `SUNNYCLOUD_SYNC_*` variable is refused (helper 01 rule)

**AC-4: Cancel stops everything within 1 s (AC-52)**
Given three requests in flight through the request path against a fake that delays its answers
When `cancel()` is called
Then all three end with `Cancelled` within 1 s and the fake receives no request afterwards

**AC-5: Retry-After honoured**
Given a fake answering 429 with `Retry-After: 3`
When the request path retries
Then the next attempt is not earlier than 3 s and not later than 4 s, measured on the test clock

**AC-6: Disabled account stops work (AC-21)**
Given a fake answering 403 `account_not_active`
When any request meets it
Then the result is `AccountNotActive` and the fake receives no further request from that client

**AC-7: Tokens never printed (AC-54)**
Given credentials with sentinel values `ACC-SENTINEL` and `REF-SENTINEL` and DEBUG logging
When requests succeed, fail with each common error, and `Debug`/`Display` of `Credential` and every error are taken
Then logs, error texts and request URLs contain 0 occurrences of either sentinel, and credentials print as `***`

## Non-Functional Requirements

**Reliability**
- Cancellation is cooperative and checked between blocks and before every request.

**Security**
- No log line carries a file name, path or token (13 §4, §11).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Crate-graph and declaration search | One declaration site; no cursor method |
| AC-2 | Concurrent store use | Consistent |
| AC-3 | Binding order, defaults, range, unknown key | As stated |
| AC-5 | Retry timing on `ManualClock` | 3 s <= delay < 4 s |
| AC-7 | `Debug`/`Display` of credentials and errors | `***`; 0 sentinels |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-4 | Scripted fake (E-23/c17) with delays | `cancel()` | `Cancelled` in 1 s; silence | IT-13-18 (fake leg) |
| AC-5 | Fake 429 `Retry-After: 3` | Retry | Not before 3 s | IT-13-19 |
| AC-6 | Fake 403 `account_not_active` | Stop | No further requests | IT-13-19 |
| AC-7 | Sentinel tokens, all common failures | Logs, errors, URLs | 0 occurrences | ST-13-2, NFT-SEC-16 |

## Constraints

- Client-side traits live in `sync-core`, never in `contracts` (ADR-015, `canonical-names.md`).
- No hard-coded durations; tests use short config values and the clock helper.

## Risks & Mitigation

**Risk 1: A later operation bypasses the request path**
- *Risk*: an operation task builds its own request and skips cancellation or redaction.
- *Mitigation*: the real-server suite (E-23/c18a) re-runs cancellation and sentinel checks over every operation.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `UploadCheckpoint.state` values, the hash-state hand-off, the `ProgressSink` unit and `bearer()` when signed out are not stated (`host_traits.md` open points 1-4) | E-23 | open | Low |
| Re-estimated from 3 to 5 points: the `sync` config section was not in any child row | E-23 owner | open | Low |
| IT-13-17 is placed on E-23/c04 (key rules) and E-23/c09b (key reuse across upload retries) because both need code this task does not own; IT-13-18's real-server leg is in E-23/c18a; the hostile-name part of IT-13-19 is in E-23/c08 | E-23 | accepted-risk | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/13_sync_core/host_traits.md`, and contributes `SyncConfig`, `cancel()` and the common errors to `_docs/02_document/contracts/13_sync_core/sync_api.md`.
Consumers MUST read those files — not this task spec — to discover the interface.
