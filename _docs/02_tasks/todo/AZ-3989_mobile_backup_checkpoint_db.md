# `mobile_backup` crate skeleton and checkpoint database

**Task**: AZ-3989_mobile_backup_checkpoint_db
**Name**: mobile_backup skeleton and checkpoint DB
**Description**: Set up the shared `mobile_backup` crate as one library for Android and iOS on top of sync-core, with its typed `android` config section and an app-private SQLite checkpoint database (`asset`, `cursor`, `session`, `batch`) that only the crate writes, versioned migrations and sync-core's `CheckpointStore` over it
**Complexity**: 5 points
**Dependencies**: AZ-3985_android_uniffi_device_spike, AZ-3967_sync_host_traits_foundation, AZ-3763_sc_config, AZ-3652_initial_structure
**Component**: 17 android_app (crate `mobile_backup`)
**Tracker**: AZ-3989
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CheckpointStore`, `UploadCheckpoint`, `BatchRecord` (E-23/c14).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — checkpoint states and the library form (this task contributes them).
- `_docs/02_document/contracts/shared_config/` — helper 01 key table and typed sections (E-01/c01).
- Record of E-27/c01 (NDK, ABIs, library layout).

## Problem

Every later E-27 task, and the iOS app (18), builds on one Rust crate that runs the backup, owns the checkpoint database and is the only writer of it (17 §4, epic risk 6). The checkpoint must survive forced kills (AC-52: 10 kills over 50,000 assets with at most 1% re-hashed), and sync-core must store its large-upload resume state and small-file batch records through it, so that 13 never keeps its own copy. Without the crate, its config section and the database, nothing else in E-27 can start.

## Outcome

- The `mobile_backup` crate builds as an Android dynamic library per ABI and as a static library, depending only on sync-core and helpers (no `contracts`, no server crate, no `vault-core` in v1.0).
- A typed `android` config section from helper 01's single key table with the defaults of 17 §10.
- An app-private SQLite checkpoint database (WAL, opened lazily, single writer) with the four tables of 17 §4 and versioned forward migrations.
- sync-core's `CheckpointStore` implemented over the `session` and `batch` tables and checked against the host-traits contract cases.

## Scope

### Included
- Crate setup and library forms per the E-27/c01 record; the UniFFI scaffolding with a version probe export only (the business surface is E-27/c07).
- The `android` section: `catchup_interval` 15m, `unmetered_only` true, `charging_only` false, `hash_ahead` 8; `batch_commit_max` taken from the server's `batch_max_entries`.
- Checkpoint DB: `asset` (states `new, hashing, uploading, uploaded, skipped_gone, skipped_name, skipped_size, failed`), `cursor (key, value)`, `session`, `batch`; location supplied by the host (app-private storage); opened on first use, not at construction.
- Versioned migrations applied at open; a database written by a newer schema version is refused.
- `CheckpointStore` over the database: per-file large-upload checkpoint (`load`, `save`, `clear`) and batch records (`save_batch`, `load_batches`, `clear_batch`), with the contract's ordering invariants.
- Kill safety of every write (one transaction per logical step).

### Excluded
- The backup loop and asset transitions (E-27/c06); wire-error mapping and the UniFFI business surface (E-27/c07).
- The Kotlin project and the per-ABI CI build (E-27/c18).
- The [v1.1] iOS columns `content_identifier` and `group_id` (E-27/c06b).

## Acceptance Criteria

**AC-1: Library forms and crate graph**
Given the pinned toolchain and the E-27/c01 recipe
When the crate is built as an Android dynamic library for the recorded ABIs and as a static library on the build host with locked dependencies
Then both builds succeed, and the crate-graph check finds dependencies on sync-core and helpers only, with no `contracts`, no server crate and no `vault-core`

**AC-2: Schema and migrations**
Given an empty app-private directory
When the store opens
Then the four tables exist at the current schema version; a second open applies nothing; a database at an older version is migrated forward with its rows intact; a database at a newer version is refused with a typed error and left unchanged

**AC-3: Lazy open**
Given a newly constructed core
When no operation that needs the checkpoint has run
Then no database file has been created or opened

**AC-4: `CheckpointStore` contract**
Given the SQLite store and the `MemoryCheckpointStore` of sync-core
When the host-traits contract cases run on both (save, load, clear; `save_batch` before every `stage_small` of the batch; `load_batches` after a restart; `clear_batch`)
Then both give the same results, and every record survives a process restart of the SQLite store

**AC-5: Kill safety**
Given a native test binary that writes 10,000 asset rows and batch records, one transaction per step
When it is killed with SIGKILL 10 times at random moments and reopened each time
Then every reopen succeeds and passes the integrity check, every committed transaction is present and no partial one is

**AC-6: Size**
Given 50,000 asset rows with hashes, names and node ids filled
When the database is checkpointed
Then the file is under 10 MB

**AC-7: Config defaults**
Given helper 01's key table
When the `android` section loads with no overrides
Then the values are `catchup_interval` 15m, `unmetered_only` true, `charging_only` false, `hash_ahead` 8, and an unknown `android.*` key refuses the load with the key named

## Non-Functional Requirements

**Reliability**
- WAL mode; one writer (the crate); Kotlin and Swift read state only through `BackupCore`.

**Compatibility**
- `rusqlite` with bundled SQLite at the workspace pin; same schema on Android and iOS.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Open empty, reopen, migrate old, refuse newer | Tables at version; rows kept; newer refused |
| AC-3 | Construct without use | No file created |
| AC-4 | Host-traits contract cases on SQLite and memory stores | Same results |
| AC-7 | Defaults and an unknown key | Defaults; unknown key named |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Clean checkout | Both library builds and crate-graph check | Builds pass; no forbidden edges | ADR-013, ADR-015 |
| AC-4 | Store with records | Process restart, reload | Records intact | AC-52 |
| AC-5 | 10,000 rows, 10 SIGKILLs | Reopen and integrity check | All reopen; no partial transaction | AC-52 |
| AC-6 | 50,000 rows | File size | Under 10 MB | |

## Constraints

- The checkpoint belongs to 17; sync-core keeps no persistent state of its own (ADR-015, epic risk 6).
- Config keys come only from helper 01's table (cross-cutting rule); no local config parser.

## Runtime Completeness

- **Capability**: the persistent checkpoint store of the mobile uploader (SQLite through `rusqlite`, bundled).
- **Production code that must exist**: the SQLite-backed `CheckpointStore` and the migrations used by every Android and iOS build.
- **Allowed stubs**: `MemoryCheckpointStore` in unit tests of other tasks only.
- **Unacceptable substitutes**: an in-memory or file-per-record store in a release build; a second checkpoint inside sync-core.

## Risks & Mitigation

**Risk 1: 13's checkpoint record does not fit 17's table**
- *Risk*: the `UploadCheckpoint` record of the host-traits contract (`session_id`, `block_hashes`, `file_hash`, `commit_id`, `state`) has more fields than 17 §4's `session (asset_id, session_id, block_bitmap_hint)`.
- *Mitigation*: the store keeps what the contract requires; the implementer records the column change in 17 §4 (see Flagged concerns).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 17 §4's `session` table lacks fields of 13's `UploadCheckpoint` (`block_hashes`, `file_hash`, `state`); the store follows the host-traits contract and 17 §4 must be updated to match | E-27 / E-23 owners | open | Medium |
| How Kotlin reads `android.*` values it needs for WorkManager (`catchup_interval`, `unmetered_only`, `charging_only`) is not stated; `BackupCore` has no config getter | E-27 owner | open | Low |
| Keys are named `android.*` while 18 §10 expects `mobile.*` for the shared keys | E-27 / E-28 owners | open | Low |
