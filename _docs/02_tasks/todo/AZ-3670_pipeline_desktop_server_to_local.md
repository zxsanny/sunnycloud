# Desktop feed loop, apply and first-sync merge, server to local (F9, F7, F14)

**Task**: AZ-3670_pipeline_desktop_server_to_local
**Name**: Desktop server-to-local pipeline
**Description**: One feed loop per account long-polls the server, and the change applier and first-sync merger write verified downloads, moves, trash and conflict copies safely into the sync roots, end to end against the real server
**Complexity**: 5 points
**Dependencies**: AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3893_platform_fs_windows, AZ-3901_decide_from_disk_pair_worker, AZ-3902_directory_move_case_roots, AZ-3903_delete_guard_holds, AZ-3904_feed_loop, AZ-3905_change_applier, AZ-3906_first_sync_merger, AZ-3918_hostile_names_escape, AZ-3960_sync_tree_operations, AZ-3964_verified_download, AZ-3965_feed_client, AZ-3657_pipeline_download, AZ-3658_pipeline_change_feed, AZ-3666_pipeline_db_restore_resync, AZ-3668_pipeline_desktop_device_login, AZ-3652_initial_structure; notes: E-31/c16 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon (daemon binary, runtime root)
**Tracker**: AZ-3670
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/p03

## Problem

The remote half of F9 and the client side of F7 and F14 chain the feed loop (one per account), the change applier with its dirty-path rule, sync-core's verified download, the platform layer's handle-relative writes and trash, the echo record, the name escaping and the first-sync merger. E-24 tests each with fakes. No planned task starts the feed loop in the running daemon and proves that server changes land locally once, safely, and that a resync after a restore loses nothing.

## Outcome

- Changes made on the server by other clients appear in the sync roots within the AC-5 budget, never echo back, and never overwrite unsynced local work.
- A first pair connection and a `resync_required` both run the merge rule with no delete sent.
- Every component operation of this half has a production caller; at least one integration test runs it with the real daemon binary against the real server stack.

## Scope

### Included
- Flow order and seams (`system-flows.md` F9 steps 8-9, F7 step 4, F14 steps 4-7, client side):
  1. Runtime root (E-24/p01) → one feed loop per account with the cursor from the index.
  2. Feed loop → `SyncApi::poll_changes` → items fanned out to pairs by ancestry.
  3. Change applier: dirty, conflict, delete-held or in-flight entries are not overwritten; downloads through `SyncApi::download` into a `DownloadSink` (verified blocks) → temp file in the root through directory handles → echo record → `rename_over_in`; remote delete → `trash`; moves applied as moves; unrepresentable names saved `%XX`-escaped.
  4. Cursor saved only after apply.
  5. `ResyncRequired` → `SyncApi::head_cursor` and `list_children` in pages → first-sync merger; the same merger on `add_pair` for a non-empty folder.
- The end-to-end integration test of the steps above.

### Excluded
- Local-to-server uploads (E-24/p02), the server feed itself (E-22/p06), macOS (v1.1).

## Acceptance Criteria

AC-1..AC-7 are local to this task. No interaction risk of `interaction-risks.md` is owned here (AC-69 is the server-side feed in E-22/p06).

### Wiring and flow criteria

**AC-1: Remote change lands locally once**
Given a paired folder and another client committing a file through the real server
When the daemon's feed loop receives the change
Then the file appears in the root (temp file inside the root, then atomic rename) and is never uploaded back

**AC-2: Remote delete, move and conflict copy**
Given synced files and a local file with unsynced edits
When the server deletes one file, moves another, creates a conflict copy of a third, and changes the locally edited file
Then the deleted file goes to the OS trash, the moved file moves, the conflict copy appears beside the original, and the locally edited file is not overwritten (the local version uploads first with its base revision)

**AC-3: Unrepresentable names**
Given server names the local filesystem cannot hold (reserved, traversal, bidi, case collision)
When they are applied
Then each is saved under a `%XX`-escaped name and listed by IPC `list_escaped`

**AC-4: First-sync merge on pair add**
Given a non-empty local folder and a non-empty server folder with local-only, server-only, equal and different files
When the pair is added
Then local-only files upload, server-only files download, equal files link with 0 bytes sent, and different files upload the local content as a conflict copy and download the server version

**AC-5: Resync after an epoch bump**
Given a synced daemon and a server restored and bumped (E-22/p13 procedure)
When the daemon receives `resync_required`
Then it reads the head and lists the tree, merges with the same rule, sends no delete, and resumes polling from the new cursor

**AC-6: Crash during apply**
Given a batch of 100 remote changes being applied
When the daemon is killed mid-batch and restarted
Then each change is applied exactly once and the cursor is only past changes that were applied

**AC-7: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds the runtime root spawning one feed loop per account, production callers of `SyncApi::{poll_changes, head_cursor, list_children, download}`, the change applier calling the platform layer's `create_new_in`, `rename_over_in` and `trash`, and the first-sync merger called on pair add and on `ResyncRequired`

## Non-Functional Requirements

**Performance**
- A remote change of up to 10 MB visible locally within 5 s at p95 (AC-5); measured on LIN-VM and WIN-VM in E-31/c43.

**Compatibility**
- Linux in CI; Windows recycle bin and handle-relative writes on WIN-VM (lab).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-7 | Static caller check over non-test sources | Each named call and spawn has a production caller |

## Blackbox Tests

Integration through the real daemon binary and the real server stack; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Paired folder, remote commit | Feed apply | File present, not re-uploaded | AC-5, AC-18 |
| AC-2 | Synced files, one locally edited | Remote delete, move, conflict, edit | Trash, move, sibling copy, local kept | AC-8, AC-31 |
| AC-3 | Hostile server names | Apply | Escaped and listed | AC-37 |
| AC-4 | Mixed local and server folders | Add pair | Upload, download, link 0 bytes, conflict copy | |
| AC-5 | Restored and bumped server | Resync | Merge, no delete, polling resumes | AC-41 |
| AC-6 | 100 remote changes | Kill mid-batch, restart | Once each, cursor safe | |

## Constraints

- Owner: component 14 (`src/desktop/daemon/`): feed-loop spawning and this half's end-to-end tests among the daemon's integration tests.
- No fake of sync-core or the server; the OS layer is real in the desktop-linux containers (Windows on WIN-VM).
- Downstream: E-31/c24 (FT-P-13, FT-P-14 device side), E-31/c29 (FT-P-48..60) and E-31/c40 should depend on this task.

## Risks & Mitigation

**Risk 1: Cursor saved before apply**
- *Risk*: a crash after the cursor write loses changes silently.
- *Mitigation*: AC-6 kills the daemon mid-batch.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Windows reserved-name marker (`%5F`) and the ` (2)` collision suffix are open owner items (architecture.md) and decide AC-3's expected names | Owner | open | Low |
