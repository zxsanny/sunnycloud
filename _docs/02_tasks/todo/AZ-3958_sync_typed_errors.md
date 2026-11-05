# Sync core typed error model and wire-code mapping table

**Task**: AZ-3958_sync_typed_errors
**Name**: Typed errors and wire-code table
**Description**: The client's typed error set and the single table that turns every `application/problem+json` code into a typed error or a planner input
**Complexity**: 3 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3764_sc_error
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3958
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — typed errors and the wire-code table (this task contributes that part).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — error body and status rules (draft, E-21/c09).
- `_docs/02_document/contracts/shared_error/sc_error.md` — codes, status table, `ApiError::parse` (E-02/c01).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — planner result types.

## Problem

Hosts must react to defined outcomes (pause on quota, skip a too-large file, re-list a vanished parent, resync) and must never parse HTTP themselves. 13 §2 says the mapping table is the only place wire codes become typed errors; without it each operation would map its own statuses and hosts would see different errors for the same failure. The server codes are snake_case in `sc_error`; 13 names its typed errors in its own words, so the table must be written once and tested row by row.

## Outcome

- One typed error set: the common errors (`Transport`, `Cancelled`, `UpgradeRequired`, `Unauthorized`, `RateLimited{retry_after}`, `AccountNotActive`, `Unexpected{code, request_id}`) plus the per-operation errors of the `sync_api` contract.
- One mapping function from a parsed problem body (through `ApiError::parse`) to a typed error or a planner input, covering every row of the contract's table.
- A row-by-row test and a property test that the mapping is total.

## Scope

### Included
- The typed error enums and their safe `Display` texts (no token, name or path).
- The mapping table, including planner inputs (`BlockConflict`, `HashRejected`, `SessionGone`, `NeedBlocks`) and `ResyncRequired` for 410 and 422 `invalid_cursor` on the feed.
- Carrying `retry_after`, `need_blocks`, `reason`, `min_version`/`update_url` and `request_id` from the body into the typed values.
- Unknown codes to `Unexpected{code, request_id}`.

### Excluded
- Sending requests and retry timing (E-23/c07, c14); 426 handling inside `connect` (E-23/c07 uses this table).
- New server codes (E-02 and the owning epics add rows to `sc_error`).

## Acceptance Criteria

**AC-1: Every row maps as documented (AC-64)**
Given a scripted problem body for every row of the `sync_api` wire-code table
When the mapping runs
Then each body yields the row's typed error or planner input with its parameters (`retry_after`, `need_blocks`, `reason`, `request_id`) carried over

**AC-2: Unknown codes keep their identity**
Given a body with code `weird_code` and request id `r-1`
When the mapping runs
Then the result is `Unexpected{code: "weird_code", request_id: "r-1"}`

**AC-3: 426 is never retried as a transient error**
Given a 426 `upgrade_required` body with `min_version` and `update_url`
When the mapping runs
Then the result is `UpgradeRequired{message, update_url}` and the error is classified as not retryable

**AC-4: Resync from both statuses (AC-41)**
Given a 410 `resync_required` with each `reason` and a 422 `invalid_cursor` from the feed
When the mapping runs
Then the results are `ResyncRequired{EpochChanged | BelowPurgeHorizon | CursorAhead}` and `ResyncRequired{InvalidCursor}`

**AC-5: Total and safe**
Given random bodies, statuses and headers
When the mapping runs
Then it never panics, and no `Display` or `Debug` text of a typed error contains a planted token, file name or path

## Non-Functional Requirements

**Reliability**
- The mapping is total; coverage of the table is 100% (critical path: wire-code table, E-23 Testing Strategy).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Every table row | Typed error per row (IT-13-13) |
| AC-2 | `weird_code`, `r-1` | `Unexpected` with both values (IT-13-13) |
| AC-3 | 426 body | `UpgradeRequired`, not retryable |
| AC-4 | 410 per reason, 422 `invalid_cursor` | `ResyncRequired` variants (IT-13-12) |
| AC-5 | Fuzzed bodies; planted secrets | No panic; no leak (ST-13-2) |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Scripted fake server answers each row (E-23/c17) | Typed error at the API surface | As table | IT-13-13, FT-N-27 |

## Constraints

- One table, one place (13 §2); operations never interpret statuses themselves.
- Codes and statuses come from `sc_error`; this task adds no code of its own.

## Risks & Mitigation

**Risk 1: Table drift against the server**
- *Risk*: a server code changes status or name and the client maps it wrongly.
- *Mitigation*: the table is keyed by `sc_error` codes, so a renamed code fails to build; the real-server suite (E-23/c18a) exercises the main rows.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `ApiError::parse` turns unknown codes into `internal`, losing `weird_code`; AC-2 needs the raw code kept (`sync_api.md` open point 1) | E-02 owner | open | Medium |
| `EditedSince{current}` has no source: the problem body carries no current revision id (`sync_api.md` open point 2) | E-02 / E-21 / E-14 owners | open | Medium |
| `InvalidCode` and the RFC 8628 device-poll answers have no `sc_error` rows yet (`sc_error.md` open point 1) | E-15 / E-02 | open | Low |
| `batch_too_large` status 422 vs 413 undecided (`problem_details.md` open point 2); the typed result is the same either way | E-21/c02 | open | Low |
| Handling of 500/503 outside block PUTs is not stated (`sync_api.md` open point 5) | E-23 | open | Low |

## Contract

This task contributes the typed-error and wire-code part of the contract at `_docs/02_document/contracts/13_sync_core/sync_api.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
