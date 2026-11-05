# Desktop watcher cycle, local to server (F9)

**Task**: AZ-3669_pipeline_desktop_local_to_server
**Name**: Desktop local-to-server pipeline
**Description**: Watcher hints, settling, decide-from-disk, the mass-delete guard and per-pair workers turn local changes into server uploads, moves and deletes through sync-core, end to end against the real server
**Complexity**: 5 points
**Dependencies**: AZ-3881_gate_s2_100k_files_spike, AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3893_platform_fs_windows, AZ-3894_watcher_scanner_settler_rules, AZ-3901_decide_from_disk_pair_worker, AZ-3902_directory_move_case_roots, AZ-3903_delete_guard_holds, AZ-3907_placeholders_locks_hash_budget, AZ-3914_ipc_server_endpoint, AZ-3917_daemon_control_impl, AZ-3957_upload_planner, AZ-3960_sync_tree_operations, AZ-3961_sync_hash_file, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3967_sync_host_traits_foundation, AZ-3655_pipeline_large_upload, AZ-3656_pipeline_small_upload_batch, AZ-3659_pipeline_tree_conflicts, AZ-3668_pipeline_desktop_device_login, AZ-3652_initial_structure; notes: E-31/c16 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon (daemon binary, runtime root)
**Tracker**: AZ-3669
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/p02

## Problem

The local half of F9 chains the watcher and scanner, rule matcher, settler, decide-from-disk with echo suppression, placeholder and lock handling, the delete guard and the pair worker, which calls sync-core (13) for hashing, uploads, moves and deletes. Each part has its own E-24 child tested with fakes of the OS and the network. No planned task spawns the pair workers in the running daemon and proves that a real local change arrives on the real server once and safely.

## Outcome

- Local creates, edits, moves and deletes in a sync root reach the real server as the right revisions and tree changes, with settle, echo, rule and guard rules applied.
- Every component operation of this half of F9 has a production caller; the runtime root spawns one pair worker per configured pair.
- At least one integration test runs it with the real daemon binary against the real server stack.

## Scope

### Included
- Flow order and seams (`system-flows.md` F9 steps 1-8 and 10, local side):
  1. Runtime root (E-24/p01) → one pair worker per pair from the index.
  2. Start-up and periodic safety rescans → watcher hints (paths only) → rule matcher (excluded paths ignored).
  3. Settler (size and mtime unchanged for 2 s) → decide from disk with the index: update (also after rename-over), `move_node` for a directory rename, create, delete candidate; echo records suppress downloaded files.
  4. Placeholders skipped by OS attributes; locked files retried with backoff.
  5. Delete guard: root missing or empty, or 200 deletes in a batch → hold persisted → IPC `resolve_hold`.
  6. Pair worker → `SyncApi::{hash_file, upload}` (F4) or `stage_small` plus `batch_commit` (F5) with the base revision from the index; tree moves and deletes with base revisions; `CheckpointStore` over the index.
  7. IPC `update_pair` rules reach the rule matcher within 5 s.
- The end-to-end integration test of the steps above.

### Excluded
- Server-to-local apply and first-sync merge (E-24/p03), device login (E-24/p01), macOS (v1.1).

## Acceptance Criteria

AC-1..AC-6 are local to this task. No interaction risk of `interaction-risks.md` is owned here (the echo loop, mass deletes and hostile names are covered by AC-18, AC-29 and AC-37).

### Wiring and flow criteria

**AC-1: Local create and save-in-place**
Given a paired, synced folder
When a 1 MiB file and a 50 MiB file are written, and an editor saves the small file by writing a temp file and renaming it over the original
Then both files are on the server after the settle time (small through the batch path, large through the session path), and the save-in-place gives exactly one new revision on the same node id with no delete

**AC-2: Rules and live settings**
Given an exclude rule for `*.tmp` and the daemon running
When a `.tmp` file is created, then the rules are changed over IPC to also exclude `build/`, and a file under `build/` is created 5 s later
Then neither file is ever uploaded

**AC-3: Mass-delete guard**
Given 300 synced files
When 250 are deleted locally, then the hold is resolved with `RestoreFromServer`; and in a second run 199 are deleted
Then the first batch is held with nothing deleted on the server and the files come back locally; the 199 deletes reach the server with base revisions

**AC-4: Offline and restart**
Given local changes made while the network is cut, and a daemon killed mid-upload
When the network returns and the daemon restarts
Then the queued changes upload from the index, the interrupted upload resumes from its checkpoint, and no duplicate revision appears

**AC-5: Placeholders and locks**
Given an evicted placeholder and a file held locked by another process (Windows lab)
When both are in the sync root
Then the placeholder is counted as skipped and never read, and the locked file uploads within 10 s after the lock is released

**AC-6: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds the runtime root spawning one pair worker per pair, watcher and scanner hints feeding the settler, the delete guard consulted before every delete, and production callers of `SyncApi::{hash_file, upload, stage_small, batch_commit}` and the tree move and delete calls (from the pair worker), with the index-backed `CheckpointStore` passed to 13

## Non-Functional Requirements

**Performance**
- At most 100 MB RSS and 1% idle CPU at 100,000 files (AC-15); measured in E-24/c01 and E-31/c45.

**Compatibility**
- Linux in CI (desktop-linux containers); Windows on WIN-VM (lab).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over non-test sources | Each named call and spawn has a production caller |

## Blackbox Tests

Integration through the real daemon binary and the real server stack; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Paired folder | Write small and large file; rename-over save | Both uploaded; one revision same id | AC-34 |
| AC-2 | Exclude rules, IPC change | Create excluded files | Never uploaded | AC-66 |
| AC-3 | 300 synced files | Delete 250, restore; delete 199 | Hold, restored; 199 deletes sent | AC-29 |
| AC-4 | Offline changes; kill mid-upload | Reconnect; restart | Uploaded once, resumed | AC-35 |
| AC-5 | Placeholder; locked file (WIN-VM) | Sync | Skipped; uploaded within 10 s of release | AC-32, AC-33 |

## Constraints

- Owner: component 14 (`src/desktop/daemon/`): pair-worker spawning and this half's end-to-end tests among the daemon's integration tests.
- No fake of sync-core or the server; the OS layer is the real one in the desktop-linux container (Windows cases on WIN-VM).
- Downstream: E-31/c29 (FT-P-48..60) and E-31/c32 (FT-N-17..19) should depend on this task.

## Risks & Mitigation

**Risk 1: Event kinds trusted over disk state**
- *Risk*: an editor's rename-over becomes delete plus create, losing the file id and share links.
- *Mitigation*: AC-1 asserts one revision on the same node.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The settings window (15) changes settings over IPC; this task drives IPC directly, the window against the real daemon is E-25/c14's UI suite (confirm it does not use only the stub daemon) | E-25 | open | Low |
| Gate S2 (100k-file watcher and RSS) is still a spike (E-24/c01) | E-24 | open | Medium |
