# sc_error: failure taxonomy, wire codes, status table, correlation id, client parser

**Task**: AZ-3764_sc_error
**Name**: sc_error codes, tiers, status table, correlation id, client parser, silent-catch gate
**Description**: The three-tier failure taxonomy, the stable snake_case error codes with their single status table, the correlation id, the client-side problem parser and the gate that rejects silently discarded results
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure
**Component**: shared/error (helper 02)
**Tracker**: AZ-3764
**Epic**: AZ-3626 (E-02)
**Ref**: E-02/c01

## Problem

Clients must recover from defined outcomes (`name_conflict`, `need_blocks`, `resync_required`, `quota_exceeded`) and must never see internals. Without one code table each component invents codes and statuses, the OpenAPI document drifts and share-link failures stop being byte-identical (AC-12). Silent catches hide failures.

## Outcome

- Business code returns typed values from one taxonomy and never knows a status; the status of every code comes from one table.
- Missing ids, cross-account ids and every missing, expired or revoked share go through one constructor and produce identical bytes.
- Clients parse every error body to a typed value, unknown codes included, and keep the request id.
- A seeded silent discard of a `Result` fails the PR gate.

## Scope

### Included
- `ErrorCode` with every code of helper 02's table and its status as plain integers (no HTTP crate).
- Tier types: validation problems, `BusinessError` with typed parameters (`need_blocks`, `retry_after`, `reason`, `min_version`), unexpected with source and correlation id, and the `DomainError` sum.
- The shared `not_found` constructor.
- `CorrelationId`: parse of an `X-Request-Id` value, a fresh UUIDv7 from a caller-supplied generator when missing or malformed.
- The problem body schema (field names as in the contract; wire name `request_id`).
- `ApiError::parse` behind the `client` feature, including transport failures; native and `wasm32-unknown-unknown` builds with that feature.
- The silent-catch gate: a test that fails on an empty catch-all or a `let _ =` on a `Result`, on top of the workspace lints AZ-3652 set (`unused_must_use`, `clippy::let_underscore_must_use` deny).

### Excluded
- The HTTP mapper, headers and OpenAPI generation (E-21).
- Per-operation outcome enums (each component's epic).

## Acceptance Criteria

**AC-1: One status per code**
Given the full code list
When the table test enumerates it
Then every code has exactly one status, 0 codes are unmapped, and every wire string is unique snake_case

**AC-2: Quota is 507 (AC-39)**
Given `quota_exceeded` and `free_space_floor`
When their status is read
Then both are 507

**AC-3: Identical not-found (AC-12, AC-59)**
Given errors built through the shared constructor for a missing id, a cross-account id, and a missing, an expired and a revoked share link
When each is serialised with the same request id
Then the five bodies are byte-identical

**AC-4: Tier 3 is opaque**
Given an unexpected error whose source contains a planted secret, an SQL fragment and a file path
When it is serialised
Then the body carries code `internal` and the request id only; 0 of the planted strings appear (NFT-SEC-38 step 3)

**AC-5: Retry information travels (AC-20)**
Given `rate_limited` and `overloaded` errors built with a retry delay
When they are serialised and parsed back
Then both carry `retry_after` with the same value

**AC-6: Client parse is total**
Given fuzzed bodies, an empty body, a body with code `brand_new` and a transport failure
When `ApiError::parse` handles each
Then it never panics, `brand_new` becomes `internal` with the request id kept, and the transport failure becomes `Transport(kind)`

**AC-7: Silent discard rejected**
Given a seeded `let _ = result;` on a `Result` in a workspace crate
When the lint step and the silent-catch test run
Then the PR gate fails

**AC-8: Builds for the browser**
Given the `client` feature
When the crate builds for `wasm32-unknown-unknown`
Then it exits 0

## Non-Functional Requirements

**Performance**
- Building a tier-2 error captures no backtrace and allocates nothing beyond the value.

**Compatibility**
- Dependencies `serde`, `serde_json`, `thiserror`, `uuid`; features `server` (default) and `client`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Code table enumeration | Unique codes, one status each |
| AC-3 | Byte comparison of the five not-found bodies | Identical |
| AC-4 | Serialisation of an unexpected error with planted secrets | No leak |
| AC-5 | Round trip `BusinessError` -> body -> `ApiError::parse` | Same code and parameters |
| AC-6 | Fuzz `ApiError::parse` | No panic; unknown code is `internal` |
| AC-7 | Silent-catch test on a seeded violation | Fails |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-3 | Share links: missing, expired, revoked (through E-21) | FT-N-09, NFT-SEC-17 | Identical not-found responses | — |
| AC-3 | Two-account matrix (through E-21) | NFT-SEC-13 | 404 with identical body for foreign ids | — |
| AC-5 | Abuse limits exceeded (through E-21) | NFT-SEC-04, NFT-SEC-05, NFT-SEC-06 | 429 with `Retry-After` | — |
| AC-1 | Old client version (through E-21) | FT-N-27 | 426 `upgrade_required` | — |
| AC-1 | CSRF failure (through E-21) | NFT-SEC-09 | 403 `forbidden` | — |
| AC-4 | 20 malformed requests (through E-21) | NFT-SEC-38 step 3 | No internals in bodies | — |

## Constraints

- One failure strategy project-wide; hot per-item paths return values, they do not throw (helper 02).
- The wire field is `request_id`; the Rust type is `CorrelationId` (E-02 risk 3).

## Risks & Mitigation

**Risk 1: Status table drifts from OpenAPI**
- *Risk*: the OpenAPI document shows other statuses than the table.
- *Mitigation*: E-21 generates OpenAPI from this table and diffs it in CI.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Codes used by 08 (`lease_lost`, `revision_obsolete`, `thumbnail_not_ready`, `thumbnail_unavailable`, output too large, type or kind rejected), 20 (412 precondition failed, 428 missing `If-Match`) and 01 (RFC 8628 poll codes) are not in helper 02's table. This task implements the table as written; each owning epic adds its rows. | Owners of E-15, E-18, E-30 | open | Medium |
| Helper 02 shows the body field as `correlation_id`; E-02 fixes the wire name `request_id`. Helper 02 needs a doc fix. | E-02 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_error/sc_error.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
