# DeleteGuard, root sentinel and hold persistence

**Task**: AZ-3903_delete_guard_holds
**Name**: Delete guard and holds
**Description**: Count deletions per batch in both directions and hold the pair when 200 or more are pending or when the root is missing, unmounted (sentinel mismatch) or empty while the index is not; persist the hold across restarts, keep uploading new files during it, and turn `resolve_hold` into a released or restored set of paths
**Complexity**: 3 points
**Dependencies**: AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3763_sc_config
**Component**: 14 desktop_daemon
**Tracker**: AZ-3903
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `HoldView`, `HoldKind`, `HoldDecision`, `NoSuchHold` (E-24/c05).
- `_docs/02_document/contracts/shared_config/sc_config.md` — key rows (E-01/c01).

## Problem

An unplugged drive, an emptied folder or a mistaken mass delete looks to a sync client exactly like "the user deleted everything". Propagating that destroys data on every device (AC-29, R24). The daemon must stop deletes before they leave, in both directions (local deletes to the server, server deletes to local files), until the user decides, and the decision must survive a restart.

## Outcome

- Every delete candidate, local or remote, passes the guard; descendants of a deleted directory are counted individually.
- `desktop.mass_delete_threshold` (200) or more deletes pending in a batch puts the pair in `hold_mass_delete`; 199 pass without a prompt.
- At start and on every rescan: a root that is missing, unmounted (root sentinel file or id mismatch) or empty while the index is not puts the pair in `root_missing`; nothing is deleted in either direction.
- `hold_detail` (kind and the held paths with their direction) is persisted in the index and restored after a restart.
- During a hold, new files keep uploading.
- `resolve_hold`: `ConfirmDeletes` releases the held deletes to their senders (local deletes to the pair worker, E-24/c10a; remote deletes to the change applier, E-24/c12b); `RestoreFromServer` hands the held paths to the change applier for re-download (E-24/c12b); the pair returns to `active`. No hold gives `NoSuchHold`.
- The `desktop.mass_delete_threshold` row added to helper 01's key table; WARN logged when a hold is entered.

## Scope

### Included
- Counting, thresholds, root checks, the sentinel written at `add_pair`, hold state, persistence, hold views for `DaemonStatus` (counts per direction), and the resolve transition.

### Excluded
- Sending deletes (E-24/c10a) and trashing or re-downloading files (E-24/c12b); the tray and settings prompts (E-24/c17, E-25).

## Acceptance Criteria

**AC-1: 199 pass, 200 hold (AC-29)**
Given the production threshold 200
When 199 local deletes arrive in one batch, and separately 200 local deletes, and separately 200 remote deletes
Then the 199 are released with no hold; each 200 case puts the pair in `hold_mass_delete` with 0 deletes released in that direction

**AC-2: Directory descendants counted individually**
Given a deleted directory with 200 descendant files
When the guard counts the batch
Then the count is 200 and the pair is held

**AC-3: Missing, unmounted or empty root**
Given a pair whose root is renamed away, one whose sentinel id no longer matches (volume swapped), and one whose root is empty while the index holds entries
When the daemon starts or rescans
Then each pair is `root_missing` and 0 deletes are released in either direction

**AC-4: Hold survives restart**
Given a pair in `hold_mass_delete`
When the daemon is restarted
Then the pair is still held with the same `hold_detail`, and `status.holds[]` shows its kind and both counts

**AC-5: New files during a hold**
Given a held pair
When a new file is created in it
Then the new file is uploaded while the deletes stay held

**AC-6: Resolving the hold**
Given a held pair
When `resolve_hold` is called with `ConfirmDeletes`, and in a second run with `RestoreFromServer`, and once on a pair with no hold
Then `ConfirmDeletes` releases exactly the held deletes to their senders, `RestoreFromServer` hands exactly the held paths to the applier for re-download and releases none, both return the pair to `active`, and the no-hold call returns `NoSuchHold`

## Non-Functional Requirements

**Reliability**
- No code path deletes a local file or a server node without a guard release.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | 199 / 200 local / 200 remote | Pass; hold; hold |
| AC-2 | Directory with 200 descendants | Count 200; hold |
| AC-3 | Renamed root, sentinel mismatch, empty root with non-empty index (fake `PlatformFs`) | `root_missing`; 0 releases |
| AC-4 | Persist, reopen index, read hold | Same `hold_detail` and counts |
| AC-6 | Resolve both decisions; no hold | Released set / restore set; `NoSuchHold` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-6 | Server; production threshold; `e2e-ctl` unmount (Linux), VHD dismount (H-WIN); restart | IT-14-5, AT-14-3 steps 1-2 | 199 propagate; 200 either way held, 0 `delete_node`, 0 local deletions; root cases `root_missing`; hold survives restart; new files upload; `NoSuchHold` | AC-29, FT-N-17, FT-N-18, FT-P-59, NFT-RES-11, NFT-RES-12 |

The end-to-end effects of a resolution (deletes sent with base revisions; 200 files re-downloaded with equal hashes) are asserted by E-24/c10a AC-7, E-24/c12b and E-24/p02 AC-3.

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- Unit coverage target 90% on this module (E-24 Testing Strategy).

## Risks & Mitigation

**Risk 1: A batch boundary splits a mass delete**
- *Risk*: 300 deletes arrive as two batches of 150 and pass.
- *Mitigation*: a batch is everything pending for the pair at decision time, including a rescan's whole diff.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| What `RestoreFromServer` means for a hold caused by 200 or more remote deletes is not defined | E-24 with owner | open | Medium |
| An emptied root (intended by the user) has no confirm path: the mockup offers no button for "Folder missing" | E-24 with owner | open | Low |
| What exactly "one batch" is (time window or pending set) is not defined; this task uses the pending set at decision time | E-24 | open | Low |
