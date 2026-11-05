# Transfer crate skeleton, tables, config and progress table

**Task**: AZ-3733_transfer_skeleton
**Name**: Transfer skeleton
**Description**: Wire the transfer crate to its helpers and ports, define its typed errors and config section, create `upload_sessions` and `commit_receipts`, and provide the in-memory progress table type
**Complexity**: 3 points
**Dependencies**: AZ-3769_contracts_part2, AZ-3740_migrations_setup, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3786_sc_observability, AZ-3723_blobstore_skeleton, AZ-3652_initial_structure
**Component**: 03_transfer
**Tracker**: AZ-3733
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.

## Problem

Every transfer task needs the same base: the session and receipt tables with their uniqueness rules, typed errors with stable problem codes, the config keys with their defaults, and the in-memory progress table that holds received bits and block hashes between checkpoints. The progress table is deliberate state (decision 12) and must stay within its memory bound.

## Outcome

- `upload_sessions` and `commit_receipts` exist as `data_model.md` §6.3 and §7.3 describe, with grants and row-level security.
- The transfer errors map to the stable codes and statuses of 03 §4 through helper 02.
- The config keys of 03 §6 load with their defaults; block size stays a protocol constant.
- A progress table type keeps received and in-flight bits and block hashes per session, safe under 4 concurrent lanes, and can be filled from a checkpoint row.

## Scope

### Included
- Migrations: `upload_sessions` (`blob_id` UNIQUE, `UNIQUE(account_id, client_key)`, `(expires_at)`, CHECK `declared_size >= 0`) and `commit_receipts` (PK `(account_id, commit_id)`, `(created_at)`), grants (DML only), RLS.
- Typed `TransferError` with codes `quota_exceeded` 507, `free_space_floor` 507, `size_too_large` 413, `session_cap_exceeded` 409, `staged_cap_reached` 409, `commit_id_reused` 422, `batch_too_large` 422, `blocklist_mismatch` 422, `account_not_active` 403, `parent_not_found` 404, `not_a_folder` 422, `parent_deleted` 410, `block_hash_mismatch` 422, `block_length_mismatch` 422, `block_conflict` 409, `need_blocks` 409, `edited_since` 409, `content_unavailable` 404, `range_not_satisfiable` 416, `upload_session_gone` 410, `overloaded` 503.
- Config: `upload_session_ttl` (24 h rolling), `max_declared_size` (50 GiB), `max_open_sessions` (8), `max_staged` (1000), `checkpoint_bytes` (8 MiB), `checkpoint_interval` (1 s), `max_lanes` (4), `max_inflight_blocks_per_account` (32), `batch_max_entries` (500), `commit_receipt_ttl` (48 h), `max_buffered_block_bytes` (1 GiB), session reap interval (1 min), receipt janitor interval (1 h); a later owner decision in E-14/c01 may change the lane or checkpoint defaults.
- Progress table type: per-session lock, received bitset, in-flight bitset, block hashes; filled lazily from a checkpoint row, never rehydrated at start.

### Excluded
- Any upload, commit or download operation (later E-14 tasks).

## Acceptance Criteria

**AC-1: Tables apply and revert**
Given the E-06 accounts migration and the E-10 `blobs` table
When the transfer migrations run up, down and up as `sunnycloud_migrator`
Then both tables exist with the keys, indexes and CHECKs in Scope, RLS is on, and `sunnycloud` has DML only

**AC-2: Client key unique per account**
Given account A with a session for `client_key` k
When A inserts a second session with k, and account B inserts one with k
Then A's insert is refused and B's succeeds

**AC-3: Error codes**
Given each `TransferError` variant
When it is converted through helper 02
Then it yields exactly the code and status listed in Scope

**AC-4: Config defaults and overrides**
Given no transfer keys set, and separately `upload_session_ttl` set to 400 ms
When the config loads
Then the defaults in Scope apply, the override yields 400 ms, and no key changes the 2 MiB block size

**AC-5: Progress table under lane races**
Given one session of 24 blocks
When 4 tasks set received bits and hashes for random blocks concurrently, 10% of them twice
Then no bit or hash is lost or overwritten with another block's value

**AC-6: Memory bound**
Given 100 sessions declaring 50 GiB each (25,600 blocks per session)
When their progress entries are allocated
Then the table's memory is at most 80 MB (fail above 100 MB)

## Non-Functional Requirements

**Performance**
- Progress table memory per AC-6 (PT-03-5).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Variant-to-code table | Every variant maps as listed |
| AC-4 | Defaults, overrides | Listed values; ms override |
| AC-5 | Bitmap and lane races | No lost bit or hash |
| AC-6 | Entry size for 25,600 blocks | Within bound |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty PostgreSQL 18 | Up, down, up; grant probe | Schema as specified | NFT-SEC-46 |
| AC-2 | Accounts A, B | Duplicate `client_key` inserts | Refused for A; allowed for B | AC-50 |
| AC-6 | 100 declared sessions | Allocation | <= 80 MB | PT-03-5 |

## Constraints

- Migrations use the `transfer` domain word (module-layout § Migrations); port signatures come from `contracts` (E-00/c04).

## Risks & Mitigation

**Risk 1: In-memory state with replicas (R06)**
- *Risk*: two API processes split one session's lanes.
- *Mitigation*: v1.0 runs one API process (deployment docs); the checkpoint makes a restart safe.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `upload_session_gone` (410) and `UploadNotFound` coexist; which code `UploadNotFound` maps to is not stated | E-14 owner, E-21 | open | Low |
| 409 vs 412 for `need_blocks` and `edited_since` is open (G-11-1, E-21/c02) | E-21 | open | Low |
| G-03-1 may change `max_lanes` or checkpoint defaults (E-14/c01) | Owner | open | Medium |
