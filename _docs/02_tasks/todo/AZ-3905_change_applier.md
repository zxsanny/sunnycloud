# ChangeApplier: dirty-path rule, verified downloads, moves and trash

**Task**: AZ-3905_change_applier
**Name**: Change applier
**Description**: Apply one feed row to one pair safely: never overwrite, move or delete a dirty, conflict, delete-held or in-flight path (the local change uploads first), write downloads to a temp file inside the root, verify the hash, write the echo record, then rename into place; apply moves by node id, send remote deletes through the guard to the OS trash, and re-download held paths on `RestoreFromServer`
**Complexity**: 5 points
**Dependencies**: AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3894_watcher_scanner_settler_rules, AZ-3901_decide_from_disk_pair_worker, AZ-3903_delete_guard_holds, AZ-3964_verified_download, AZ-3967_sync_host_traits_foundation
**Component**: 14 desktop_daemon
**Tracker**: AZ-3905
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c12b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `DownloadSink` (`write_verified`, `finish`) (E-23/c14).
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `download` and its errors (E-23/c11 contribution).

## Problem

A server change must never destroy unsynced local work: an edit made offline must win or become a conflict copy, never be overwritten (AC-8, FT-P-77). An unverified download must never replace a local file (AC-7). A remote delete must not unlink (R24). Moves must keep node ids so that an edit inside a moved folder lands on the same node (AC-70). And the applier's own writes must not echo back as uploads (AC-18).

This is the second half of the split E-24/c12 row (see E-24/c12a).

## Outcome

- The dirty-path rule: a row for a path whose entry is `dirty`, `conflict`, `delete_held` or has an `upload_checkpoint` row is deferred; the local change uploads first with its `base_revision` (through E-24/c10a); after its result is stored the row is applied from the re-read cursor (E-24/c12a).
- Downloads: `SyncApi::download` into a `DownloadSink` that writes only verified blocks to a temp file created inside the root through directory handles; mtime set from the revision; entry written `synced` with `(file_id, size, mtime_ns, content_hash)` of the temp file; then `rename_over_in`.
- A hash mismatch or a kill before the rename leaves the destination unchanged and the entry not `synced`; stale temp files with the daemon prefix are removed at start.
- Moves and renames applied as moves of the local entry, keeping the node ids from the feed.
- A remote delete of a clean path goes through the guard (E-24/c11) and then `PlatformFs::trash`; on `TrashUnavailable` the file stays, the entry is marked and reported, nothing is unlinked.
- Rows for excluded paths are ignored (rule matcher of E-24/c09).
- `RestoreFromServer` from a resolved hold re-downloads the held paths through the same verified path; `ConfirmDeletes` for remote holds trashes them.
- Implements sync-core's `DownloadSink` for the daemon.

## Scope

### Included
- Per-row apply for one pair: create, update, move, rename, delete; the dirty-path rule; the verified write path; the echo record order; trash; stale temp cleanup at start.

### Excluded
- Name escaping of hostile or unrepresentable names (E-24/c20, which extends this write path); the feed loop and cursor (E-24/c12a); the guard (E-24/c11); first-sync merge (E-24/c13).

## Acceptance Criteria

**AC-1: Dirty paths are never overwritten (AC-8)**
Given entries p1 `dirty`, p2 `conflict`, p3 `delete_held`, p4 with an upload in flight, and feed updates for p1 to p4 and a delete for p1
When the rows are applied
Then p1 to p4 are untouched on disk, the local change uploads with the entry's `base_revision`, and only after its result is stored is the pending row applied (IT-14-3)

**AC-2: Remote delete to the trash (R24)**
Given a clean synced p5 and a feed delete for it, then a second clean file whose `trash` returns `TrashUnavailable`
When the deletes are applied
Then p5 is in the OS trash with 0 unlink calls, and the second file stays on disk with its entry marked and reported, 0 unlink calls

**AC-3: Unverified download never lands (AC-7)**
Given a fake `SyncApi.download` returning `HashMismatch`, and separately a kill after the temp write and before the rename
When the applier handles each
Then the destination's hash is unchanged, the entry is not `synced`, and the temp file is removed (stale temp files with the daemon prefix are removed at the next start) (ST-14-4)

**AC-4: Echo record before rename**
Given a remote change downloaded to a temp file
When the applier renames it into place
Then the entry was written `synced` with the temp file's `(file_id, size, mtime_ns, content_hash)` before `rename_over_in` (spy order), so the following watcher hint is dropped

**AC-5: Move keeps node ids (AC-70)**
Given device A moves `d` to `x/d` while device B, offline until after the move, edits `d/f.txt`
When B applies the move and uploads its edit
Then B keeps the `node_id` of `f.txt`, uploads with its base revision, and the final `x/d/f.txt` has the edit, same file id, 0 conflict copies (IT-14-22, FT-N-42)

**AC-6: Restore of held paths**
Given a pair whose 200 local deletes were held and resolved with `RestoreFromServer`
When the applier processes the held paths
Then all 200 files are back locally with hashes equal to the server's

## Non-Functional Requirements

**Reliability**
- A crash at any point leaves either the old file or the verified new file at the destination, never a partial one.

**Performance**
- A remote change up to 10 MB is visible locally within 5 s at p95 (AC-5), measured end to end in E-31.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Four protected states plus feed updates and delete (fake `SyncApi`, fake `PlatformFs`) | No disk change; upload first; row after result |
| AC-2 | Trash success and `TrashUnavailable` | Trash call; 0 unlinks; marked entry |
| AC-3 | `HashMismatch`; kill before rename | Destination unchanged; temp removed |
| AC-4 | Spy on index write and rename order | `synced` written before `rename_over_in` |
| AC-6 | Held paths handed back | 200 downloads through the verified path |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | p1..p5 states; freedesktop trash; Windows recycle bin on WIN-VM | IT-14-3 | Untouched; trash entries; 0 unlinks | AC-5, AC-8, FT-P-77, FT-P-10, FT-P-12 |
| AC-3 | Flipped block; kill between temp write and rename | ST-14-4 | No renamed file with a wrong hash | AC-7, FT-N-02 |
| AC-5 | Two daemons; move and edit | IT-14-22 | Edit on the same node in the moved folder | AC-70, FT-N-42 |
| AC-6 | 200 held local deletes, `RestoreFromServer` | IT-14-5 restore case | 200 re-downloaded, hashes equal | AC-29, FT-P-59 |

## Runtime Completeness

- Named capability: hash-verified download into place through directory handles (14 §5 step 7, §11).
- Production code that must exist: the real `DownloadSink` writing verified blocks to a temp file from `create_new_in`, and the real `rename_over_in` and `trash`.
- Unacceptable substitutes: writing to the final path and verifying afterwards; unlink as a delete.

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- Unit coverage target 90% on the dirty-path rule (E-24 Testing Strategy).
- No paths of user files above DEBUG in logs (14 §9).

## Risks & Mitigation

**Risk 1: Edit lands during a download**
- *Risk*: the user edits the path between the dirty check and the rename.
- *Mitigation*: 14 §7: the post-rename hash and mtime mismatch makes the next hint a real change, uploaded with the downloaded revision as base (conflict copy if stale, never a silent overwrite).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Where a `TrashUnavailable` entry is "reported" (which IPC list) is not named | E-24 | open | Low |
| `RestoreFromServer` for a hold of remote deletes is undefined (see E-24/c11) | E-24 with owner | open | Medium |
