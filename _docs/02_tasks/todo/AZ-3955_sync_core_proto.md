# sync-core-proto: protocol DTOs, server URL, hashing wrappers, commit-ID rules, feed-page checker

**Task**: AZ-3955_sync_core_proto
**Name**: sync-core-proto crate
**Description**: Create the sans-IO protocol crate with the wire DTOs, `ServerUrl` validation, hashing wrappers over `sc_ids`, the commit-ID rules and the feed-page invariant checker, built for native and `wasm32-unknown-unknown`
**Complexity**: 5 points
**Dependencies**: AZ-3739_sync_core_skeletons, AZ-3766_sc_ids, AZ-3764_sc_error, AZ-3767_sc_names, AZ-3652_initial_structure
**Component**: 13_sync_core (crate `sync-core-proto`)
**Tracker**: AZ-3955
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/sync-core/crate_skeleton.md` — crate boundary and the client-trait home (E-00/c07).
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — `hash_block`, `FileHasher`, `rollup`, `derive_client_key`, vector file (E-04/c01).
- `_docs/02_document/contracts/shared_error/sc_error.md` — client feature (E-02/c01).
- `_docs/02_document/contracts/shared_names/sc_names.md` — optional name preflight (E-05/c01).
- `_docs/02_document/contracts/11_http_api/server_info.md`, `api_v1_surface.md` — wire shapes (drafts, E-21).
- `_docs/02_document/contracts/05_sync_feed/change-feed.md`, `_docs/02_document/contracts/03_transfer/commit_service.md` — feed and commit shapes.

## Problem

Two executors must plan identically: reqwest in `sync-core` and `fetch` in the browser Worker (ADR-004, ADR-001). If the DTOs and pure rules live in the native client, the browser needs a second copy and the two drift; AC-3, AC-7, AC-8 and AC-52 then break in one client only. `sync-core-proto` is named by 13 and E-23 but not created by E-00 (module-layout open point 1): it does not exist yet, and every other E-23 task needs its types.

## Outcome

- A new workspace member that builds natively and for `wasm32-unknown-unknown` with no tokio, HTTP, clock or OS-randomness dependency.
- The wire DTOs, `ServerUrl`, hashing wrappers, commit-ID rules and the feed-page checker of the `sync_core_proto` contract, with their unit tests.
- The `sync_core_proto` contract moves from draft to frozen once its open points 1, 3 and 5 are answered or recorded as decided here.

## Scope

### Included
- The crate as a workspace member (the E-23 edit to the workspace manifest that `module-layout.md` names).
- DTOs: `ServerInfo`, `Cursor` (wire text `E.S`, total order), `ChangeItem`, `ChangePage`, `ResyncReason`, `NodeView`, `ListingItem`, `ListingPage`, `RevisionView`, `RevisionRef`, `UploadSession`, `StartOutcome`, `CommitOutcome`, `BatchEntry`, `BatchCommit`, `BatchResult`, `DeviceAuthorization`, `DevicePoll`, `SessionTokens`; unknown JSON fields ignored.
- `ServerUrl::parse` with the plain-HTTP refusal (the allow flag is the caller's).
- Hashing wrappers over `sc_ids` and `ResumeHashState` (hashing resumes at a block boundary).
- Commit-ID rules: mobile key derivation, stored-key reuse, batch-id check.
- The feed-page checker with rules P1..P6.
- Recording the decisions for the contract's open points 3 and 5.

### Excluded
- `UploadPlanner` (E-23/c05), the WASM bindings (E-23/c16), UniFFI derives (E-23/c15).
- Any HTTP, retry timing, token or host-trait code (E-23/c06..c14).

## Acceptance Criteria

**AC-1: Builds sans-IO for both targets**
Given the workspace
When the crate is built with `--locked` natively and for `wasm32-unknown-unknown`, and its dependency tree is checked
Then both builds exit 0 and the tree contains no tokio, reqwest, mio or clock crate, and no server crate or `contracts` (AZ-3652 crate-graph test)

**AC-2: DTOs match the wire**
Given recorded JSON examples for `ServerInfo` (`server_info.md` "fields" case), a change page, a listing page, a revision, an upload session and a batch result
When they are parsed and written back
Then every field round-trips, unknown fields are ignored, a cursor `1.150` parses to epoch 1, seq 150 and compares greater than `1.100`, and a malformed cursor text is refused

**AC-3: Plain HTTP refused (AC-54)**
Given `http://sunnycloud.test` and plain HTTP not allowed
When `ServerUrl::parse` runs
Then it returns `InsecureScheme`; `https://sunnycloud.test` is accepted; a text without a host or with another scheme is `Invalid`

**AC-4: Hashes equal `sc_ids` (AC-7)**
Given the `sc_ids` vector file
When the wrappers hash each case natively
Then every block hash and file hash equals the vector, and a 2 MiB + 1 byte block is `BlockTooLarge` (the wasm32 and UniFFI legs of the same vectors run in E-23/c16)

**AC-5: Commit-ID rules (AC-78 contribution)**
Given a stored key `K` for hash `H`, the triple `(dev-1, asset-42, H)` and a batch id equal to a member key
When the rules run
Then current hash `H` keeps `K` and `H'` yields a fresh key; the mobile key is equal for the same triple in two processes and different for `H'`; the batch check returns `EqualsMemberKey`, and a non-v7 id returns `NotV7`

**AC-6: Feed-page checker (AC-69 client side)**
Given cursor `1.100` and the pages v1..v6 of IT-13-11 and a valid page with seq 101..150, next `1.150`, `has_more = true`
When `check_page` runs on each
Then v1..v6 each fail with the violated rule named (P1..P6) and the valid page passes unchanged

## Non-Functional Requirements

**Compatibility**
- Builds with the workspace MSRV and `--locked`; same results on native and wasm32.

**Reliability**
- Parsers never panic on malformed input (property test over random JSON and cursor texts).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Dependency-tree check for both targets | No forbidden crate |
| AC-2 | DTO round trips, cursor parse and order, unknown fields | Equal; malformed refused |
| AC-3 | `ServerUrl` cases | `InsecureScheme`, accepted, `Invalid` |
| AC-4 | Vector cases native | Equal to `sc_ids` |
| AC-5 | Key reuse, mobile derivation, batch-id check | As stated (IT-13-17) |
| AC-6 | Pages v1..v6 and the valid page | P1..P6 named; valid passes (IT-13-11) |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Clean checkout | Native and wasm32 builds | Exit 0, clean tree | Compatibility |
| AC-4 | Vector file | Native hashing | 0 differences | IT-13-20 (base cases), FT-N-01 |

## Runtime Completeness

- **Named capability**: BLAKE3 block hashing, rollup and mobile key derivation (blake3 1.8.7 through `sc_ids`).
- **Production code that must exist**: the wrappers call `sc_ids` on every target; the feed-page checker runs on every real page.
- **Allowed external stubs**: none.
- **Unacceptable substitutes**: a second hash or key-derivation implementation, a checker that is skipped in release builds, fixed test hashes inside the crate.

## Constraints

- No tokio, no HTTP, no clock, no OS randomness (module-layout, ADR-004); time and randomness enter as arguments.
- Hashing and key derivation come from `sc_ids`; no second implementation (cross-cutting rule).
- Server-supplied names stay data; no DTO field is used as a local path (13 §7).

## Risks & Mitigation

**Risk 1: A default feature drags tokio into the wasm32 tree**
- *Risk*: a helper or serde-related crate pulls a native runtime.
- *Mitigation*: AC-1's tree check runs on every PR (E-00/c07 check covers `sync-core-wasm`, this adds the protocol crate).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `longpoll_max_wait` wire unit (seconds or milliseconds) not specified (`server_info.md` open point 1) | E-21 owner | open | Low |
| `ServerUrl` rules beyond "https with a host" (user info, query, fragment, path) are not specified; this task records its choice in the contract | E-23 | open | Low |
| No pure UUIDv7 constructor in `sc_ids` for the WASM "commit ID" export (`sync_core_proto.md` open point 4) | E-04 / E-23 / E-26 | open | Low |
| `UploadSession.lane_limit` versus `ServerInfo.max_lanes` precedence not stated; contract takes the lower | E-14 / E-23 | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/13_sync_core/sync_core_proto.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
