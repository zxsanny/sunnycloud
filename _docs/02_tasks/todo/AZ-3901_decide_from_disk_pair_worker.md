# Decide from disk, save-in-place, echo suppression and the pair worker's uploads

**Task**: AZ-3901_decide_from_disk_pair_worker
**Name**: Decide from disk and pair worker
**Description**: Turn each settled path into the right server action by looking at the disk and the index, not at the event kind (update of the mapped node even after rename-over, create, `move_node` for a known file id at a new path, delete candidate), drop the daemon's own downloads by their echo record, and execute the actions through sync-core with the entry's base revision, storing each typed outcome
**Complexity**: 5 points
**Dependencies**: AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3894_watcher_scanner_settler_rules, AZ-3903_delete_guard_holds, AZ-3960_sync_tree_operations, AZ-3961_sync_hash_file, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3967_sync_host_traits_foundation, AZ-3763_sc_config; notes: E-31/c22 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon
**Tracker**: AZ-3901
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c10a

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `hash_file`, `upload`, `stage_small`, `batch_commit`, `move_node`, `create_folder`, `delete_node` and their typed errors (E-23/c07 with contributors).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `FileSource`, `CheckpointStore`, `BatchRecord` (E-23/c14).

## Problem

Editors save in place by writing a temp file and renaming it over the original, or by delete and recreate. Read as events, that is a delete plus a create, which loses the file id and breaks share links (AC-34). The daemon must decide from what is on disk: an existing mapped path is an update of its node, carrying the entry's base revision, so exactly one new revision lands on the same file id. A file the daemon itself downloaded must not go back up (AC-18). The pair worker that runs these actions also needs typed outcomes, queueing while offline and a restart that never takes the whole process down.

This row grew in decomposition: the planned title had no owner for the pair worker's execution of decided actions and its typed outcome handling, so E-24/c10 is split. This task (c10a) holds the decision rules, echo suppression and execution; E-24/c10b holds directory moves, the unstable-id fallback and case-insensitive roots.

## Outcome

- Decide from disk: path exists and mapped means update of that node (also after rename-over with a new file id); unmapped path means create; a known `file_id` at a new path with the old path gone means `move_node`; a path gone after settle is a delete candidate handed to the delete guard (E-24/c11).
- Echo suppression: a hint whose `(file_id, size, mtime_ns, content_hash)` matches the `synced` record is dropped; a mismatch is a real change, uploaded with the downloaded revision as base.
- The pair worker executes actions through `SyncApi`: `hash_file` then `upload` (or `stage_small` plus `batch_commit` for small files, with batch records saved first) with `base_revision` from the index, `move_node`, `create_folder`, and `delete_node` with base revision for deletes the guard released; the index-backed `CheckpointStore` is passed to sync-core.
- Each outcome is stored: `Published` and `NothingToSend` mark the entry `synced` with the new revision; `ConflictCopy` records the server-named copy (the daemon never names a copy); network errors keep the work queued in the index with backoff; an unexpected error restarts that pair's worker with backoff while other pairs continue.
- `ConfirmDeletes` from a resolved hold sends the held local deletes with their base revisions.

## Scope

### Included
- Decision rules of 14 §5 steps 4 and 5, except directory moves and case-only renames (E-24/c10b).
- Small-file versus large-file path chosen by `small_file_max` from `ServerInfo`.
- Typed outcomes `QuotaExceeded`, `SizeTooLarge`, `SessionCapReached`, `NameConflict`, `EditedSince` stored on the entry and reported; `InvalidName` handling is E-24/c20 (FT-N-51).
- A WARN for files above `desktop.sparse_warn_bytes` (10 GiB) logical size; the key row added to helper 01's table.

### Excluded
- Directory moves, multiset fallback, case-insensitive roots (E-24/c10b); the delete guard itself (E-24/c11); feed rows and the dirty-path rule's feed side (E-24/c12b); placeholders and locks (E-24/c14).
- Spawning one worker per pair in the running daemon (E-24/p02).

## Acceptance Criteria

**AC-1: Save-in-place is one revision on the same node (AC-34)**
Given a synced file with a share link
When vim (`-es`), `keepassxc-cli`, LibreOffice UNO and a JetBrains replay save it in place (DS-15) on Linux, and Word on WIN-VM
Then per editor the server shows 1 new revision on the same node id, 0 conflict copies, 0 `delete_node` calls, and the share link still opens (IT-14-8)

**AC-2: Echo of a download is dropped (AC-18)**
Given a 1 MiB file downloaded by this daemon and recorded `synced` with `(file_id, size, mtime_ns, content_hash)`
When the watcher reports the path and nothing else happens
Then 0 upload requests are made for it (proxy log) (IT-14-2 case a)

**AC-3: A real edit with the same size and mtime is uploaded**
Given the same downloaded file
When the user rewrites it with different bytes, same size, and resets the mtime to the stored value
Then the hash mismatch makes it a real change, uploaded with the downloaded revision as `base_revision` (IT-14-2 case b)

**AC-4: Move of a known file id**
Given a synced file `a/x.txt`
When it is renamed to `b/x.txt` (same file id, old path gone)
Then one `move_node` is sent, with no upload and no delete

**AC-5: Base revisions and checkpoints passed**
Given an edit of a synced 50 MiB file and of a 1 MiB file
When the pair worker uploads them
Then the large one goes through `upload` with the entry's `base_revision` and the index `CheckpointStore`, the small one through `stage_small` plus `batch_commit` with its batch record saved before the first stage, and both entries end `synced` with the new revision

**AC-6: Offline queue and worker restart**
Given a fake `SyncApi` that returns `Transport` errors for 30 s, and separately one that panics on one pair
When local changes are made
Then the changes stay queued in the index and upload once calls succeed, with no duplicate revision; the failing pair's worker restarts with backoff and the other pair keeps syncing

**AC-7: Released deletes carry base revisions**
Given 199 local deletes (below the guard), and a resolved mass-delete hold with `ConfirmDeletes`
When the pair worker sends them
Then every `delete_node` carries the entry's base revision, and an `EditedSince` answer (edit beats delete) leaves the server file in place and the edited version comes back locally (FT-P-12)

## Non-Functional Requirements

**Performance**
- Decisions read the index and stat the disk only; no hashing before settle.

**Reliability**
- The daemon never names a conflict copy; the server does (ADR-005).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Decide on rename-over, delete-and-recreate, backup-swap sequences with fake `PlatformFs` | Update of the mapped node; no delete |
| AC-2, AC-3 | Echo record match and mismatch (IT-14-2 with spy order) | Dropped; uploaded with base; entry `synced` before `rename_over_in` |
| AC-4 | Known file id at a new path | One `move_node` |
| AC-5 | Large and small path selection; batch record order | Correct path; record saved before stage |
| AC-6 | Transport errors then success; panicking worker | Queued then sent once; restart with backoff; other pair unaffected |
| AC-7 | Delete with base revision; `EditedSince` | Base sent; server file kept, local file restored |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-15 editor scripts; Word on WIN-VM; share link | IT-14-8, AT-14-1 | 1 revision, same id, 0 copies, 0 deletes, link opens | AC-34, FT-P-57 (H-LIN/H-WIN for LibreOffice, JetBrains, Word) |
| AC-2, AC-3 | DS-23 1 MiB remote change; rewrite with `utimensat` | IT-14-2 | 0 uploads; then one upload with base | AC-18, FT-P-51 |
| AC-6 | Network cut, then restored | Offline queue | Uploaded once after reconnect | AC-35 |

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- No paths of user files above DEBUG in logs (14 §9).

## Risks & Mitigation

**Risk 1: Event kinds trusted by mistake**
- *Risk*: a rename-over becomes delete plus create.
- *Mitigation*: decisions take only a path; tests replay recorded editor sequences.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-24/c10 was split into c10a and c10b; E-24/p02 depends on "E-24/c10" and must resolve to both | Parent (Step 4) | open | Low |
| Per-file errors (`QuotaExceeded`, `SizeTooLarge`, ...) are "reported" but no IPC field or list carries them (`ipc_schema.md` open point 2; the mockup shows an "Errors" list) | E-24 with owner | open | Medium |
| "POSIX syncs only the executable bit" (14 §7): how the bit is carried (a `client_meta` field?) is not specified | E-24 / E-23 | open | Low |
