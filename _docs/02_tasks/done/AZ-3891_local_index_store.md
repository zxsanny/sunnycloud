# LocalIndexStore: schema, migrations, checkpoint side and rebuild

**Task**: AZ-3891_local_index_store
**Name**: LocalIndex store
**Description**: The daemon's SQLite index of pairs, entries, settings, the single feed cursor, upload checkpoints and small-file batch records, with forward-only migrations, typed corruption and version errors, a rebuild path, and the `CheckpointStore` that sync-core uses for resumable uploads
**Complexity**: 5 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3963_small_upload_batch, AZ-3967_sync_host_traits_foundation, AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3891
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CheckpointStore` (`load`, `save`, `clear`, `save_batch`, `load_batches`, `clear_batch`), `UploadCheckpoint`, `BatchRecord` (E-23/c14).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `Cursor`, node and revision ids (E-23/c04).

## Problem

Every daemon part reads or writes local state: watcher hints look up entries by path, rename-over looks them up by file id, feed rows by node id, holds and settings must survive restarts, and a 5 GiB upload must resume after a kill without re-hashing (AC-3). The index is never the truth: if it is lost or damaged the daemon rescans and merges, and nothing is deleted. One store with one writer must serve all of this inside the 100 MB budget.

## Outcome

- One `LocalIndexStore` over SQLite (WAL, `synchronous=NORMAL`, `foreign_keys=ON`) in the local user data directory, even when a sync root is on a share, holding `meta`, `pair`, `entry`, `batch` and `upload_checkpoint` as in data model §11; no token anywhere in it.
- Forward-only migrations by `PRAGMA user_version`; an index newer than the daemon is refused.
- The `CheckpointStore` of sync-core implemented over `upload_checkpoint` rows and batch records.
- A rebuild that replaces a damaged index with an empty one, after which the daemon rescans and runs the first-sync merge (E-24/c13) with nothing deleted.

## Scope

### Included
- Operations: `get_entry`, `upsert_entries(batch)`, `entries_in_state`, `find_by_file_id`, `find_by_node`, `set_pair_state` (with `hold_detail`), `set_cursor` (one per account), `meta_get`/`meta_set`, `rebuild`.
- Indexes of 14 §4: PK `(pair_id, rel_path)`, `(pair_id, file_id)`, `(pair_id, node_id)`, partial `(pair_id, state)` for non-synced entries.
- On case-insensitive pairs, path lookups compare components by `name_key`, so two paths that differ only in case cannot both be entries (14 §5 step 4).
- `CheckpointStore`: checkpoint saved at session start and after every `hash_checkpoint_blocks` blocks, read on restart, deleted on commit acknowledgement; batch records saved before the first stage and cleared after per-entry results are stored (`save_batch`, `load_batches`, `clear_batch`).
- One writer thread; readers do not block it.
- Errors `IndexCorrupt` (offer rebuild), `IndexNewerThanDaemon` (refuse, offer rebuild), `Io`.

### Excluded
- What rebuild triggers afterwards (rescan: E-24/c09; merge: E-24/c13); how the user is asked (tray E-24/c17, IPC E-24/c19).
- Upload logic (E-23); the meaning of entry states (E-24/c10a, E-24/c12b, E-24/c11).

## Acceptance Criteria

**AC-1: Schema and pragmas**
Given a fresh data directory
When the store opens
Then the file has WAL journal mode, `synchronous=NORMAL`, foreign keys on, the five tables and four indexes of data model §11, and `user_version` equal to the daemon's schema version

**AC-2: Lookups the pipeline needs**
Given 100,000 entries in two pairs
When entries are read by `(pair_id, rel_path)`, by `file_id`, by `node_id` and by non-synced state
Then each lookup uses its index (query plan), and the file size and resident memory are recorded against the 14 §4 measurement (about 11 MB file, 7 MB resident)

**AC-3: Case-insensitive pair keys**
Given a pair marked case-insensitive with an entry `Report.txt`
When `report.txt` is looked up and then upserted
Then the lookup finds `Report.txt` and the upsert updates that entry instead of adding a second one; on a case-sensitive pair the two are separate entries

**AC-4: Resumable upload checkpoint (AC-3)**
Given a 5 GiB upload (DS-09) whose checkpoint is saved every 32 blocks
When the daemon is killed at 40% and restarted
Then the checkpoint is read back with session id, commit id, file hash, hash state and blocklist intact, the upload re-sends at most 10 MiB, hashing CPU after restart is below 5% of the first run, and the checkpoint row is gone after the commit acknowledgement

**AC-5: Batch records survive a kill**
Given a batch record saved before its first stage
When the process is killed and the store reopened
Then `load_batches` returns the same commit id and entries, and `clear_batch` removes it after the results are stored

**AC-6: Damaged or newer index**
Given a corrupted index file, and separately an index whose `user_version` is above the daemon's
When the store opens
Then it returns `IndexCorrupt` and `IndexNewerThanDaemon` respectively, does not modify the file, and offers rebuild; after `rebuild` the store is empty and opens cleanly

**AC-7: One cursor per account**
Given two pairs of one account
When `set_cursor` is called from the feed loop
Then `meta` holds exactly one cursor row (epoch and seq) for the account

**AC-8: No secrets stored**
Given a signed-in daemon after a full sync
When every table and value of the index file is scanned
Then the device token string does not occur

## Non-Functional Requirements

**Performance**
- The index stays inside the AC-15 memory budget at 100,000 files (PT-14-1, measured with the whole daemon).

**Reliability**
- A kill at any point leaves an index that opens without `IndexCorrupt` (WAL).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Open on an empty directory | Pragmas, tables, indexes, `user_version` |
| AC-2 | Query plans of the four lookups | Each uses its index |
| AC-3 | Case-insensitive and case-sensitive upsert of `Report.txt` / `report.txt` | One entry vs two |
| AC-5 | Save, reopen, load, clear a batch record | Same content; removed |
| AC-6 | Corrupted file; higher `user_version` | `IndexCorrupt`; `IndexNewerThanDaemon`; file unchanged |
| AC-7 | Two pairs, repeated `set_cursor` | One cursor row |
| AC-8 | Scan of all values after a fake sign-in | 0 token occurrences |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-4, AC-6 | DS-09 5 GiB file, `sync.hash_checkpoint_blocks` 32; kill at 40%; corrupted file; newer version | IT-14-19 | <= 10 MiB re-sent, hashing < 5% of first run; typed errors and rebuild offered | AC-3, NFT-RES-01 |
| AC-2 | 100,000 entries (DS-11) | Index size and lookups | About 11 MB file, about 7 MB resident | PT-14-1 |

## Runtime Completeness

- Named capability: SQLite through rusqlite 0.41.1 (bundled) in WAL mode (architecture, ADR-012).
- Production code that must exist: the real schema, migrations and statements on a real SQLite file.
- Allowed external stubs: none in this task's tests; other tasks may use a fake `LocalIndexStore` at the trait boundary in unit tests (14 tests.md).
- Unacceptable substitutes: an in-memory map behind the trait in production, or `MemoryCheckpointStore` in the daemon.

## Constraints

- IT tier: the store runs in-process with the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- No paths of user files above DEBUG in logs (14 §9).

## Risks & Mitigation

**Risk 1: Writer contention**
- *Risk*: the feed loop and pair workers wait on one writer and the settle budget slips.
- *Mitigation*: batched `upsert_entries`; the one-writer rule is measured in PT-14-1.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Whether `rebuild` keeps the `pair` rows and `meta` settings (server address, settings) or the user re-adds pairs is not stated | E-24 | open | Medium |
| Data model §11 has entry state `skipped_placeholder` only; symlinks, junctions and special files are "skipped and counted" with no stored state or reason column | E-24 | open | Low |
| A name the server rejects (FT-N-51) is "marked" but no entry state or column holds it | E-24 | open | Low |
| Where the incremental hash state is kept (`host_traits.md` open point 2) decides whether `upload_checkpoint.hash_state` is written through `CheckpointStore::save` | E-23/c09a, E-23/c14 | open | Low |
