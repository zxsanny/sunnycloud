# ADR-005: Last write wins, stale edits become conflict copies, no version history or user trash

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Several devices of one account edit, add and delete the same files, some of them offline. The server has to pick one current version per file, never store a mix of two edits, and never silently drop an edit that lost the race. Deletes are the dangerous half: a missing or unmounted sync folder looks like "delete everything", and with no user trash a wrong delete would be unrecoverable on every device.

- Acceptance criteria addressed: AC-8 (later commit by server order wins, stale edit kept as "name (conflict copy)", edit beats delete), AC-29 (desktop mass-delete guard), AC-30 (retention default 7 days, 0 to 90; operator restore), AC-31 (100 name collisions yield exactly 100 conflict copies), AC-34 (save-in-place is one revision on the same file ID), AC-41 (tombstone horizon 90 days, `resync_required`), AC-42 (case-insensitive unique names), AC-70 (an edit inside a folder moved by another device yields 0 conflict copies)
- Restrictions addressed: RESTRICT-10 (retention through a GC queue, operator restore only, no user trash), RESTRICT-24 (last commit wins, no version history, edit beats delete, mass-delete guard), RESTRICT-09 (tombstones kept 90 days), RESTRICT-11 (clients send the base revision of each change)
- Risks addressed: R24 (remote delete irreversibly removes local files), R26 (rename/move without stable IDs causes a mass-delete hold), R33 (GC can unlink blobs a backup still needs), R02 (restore must not serve stale cursors)
- Research source: `_docs/01_solution/solution.md` § Component: Storage, metadata and encryption at rest (GC and retention row)

The choice is non-trivial because the safety net and the product promise pull apart. The problem statement asks for "last write wins, no version history", yet the storage review (`b_server_storage.md`, "No versions / no trash") notes that a buggy client or ransomware propagates deletes and encrypted copies to all devices, and practitioners treat versions as the real safety net. The owner accepted that trade-off; this ADR records how much protection is kept anyway.

## Decision

We will resolve concurrent changes by server commit order alone: the later commit becomes the current revision, a stale edit is preserved as a conflict copy that shares the superseded blob, an edit beats a delete, and the only recovery path is an operator-run restore inside a configurable blob retention window. Users get no version history and no trash.

**Conflicts.** Every change carries its base revision. A commit whose base is still current publishes a new revision. A commit with a stale base still becomes current; the revision it superseded is re-parented (same row, same blob, no byte copy) to a new sibling node `name (conflict copy YYYY-MM-DD HH-MM device)`, where `device` is the committing device's label and time comes from the injectable clock. A new file whose name is taken at commit gets the same conflict-copy name assigned by the server: one behaviour, no `name_conflict` on commit, no client retry. Folder create, rename and move onto an existing name are still rejected with `name_conflict`. A commit on a tombstoned file revives it and its tombstoned ancestors; a delete with a stale base is rejected with `edited_since`. "Later" is `account_feeds.seq` order, never a client clock.

**Retention and restore.** In the same transaction as a replace or delete, the API writes a `blob_gc` row due at now + retention (default 7 days, validated 0 to 90, operator only). Tombstones are kept 90 days (`tombstone_horizon`) and raise the purge horizon. `sunnycloud admin restore --account --path --at` re-points the existing revision row (`revisions.blob_id` is UNIQUE, so it never creates a second revision) and re-parents it into the original folder (`placement: Original`) or into a `Restored` folder at the root (`placement: Restored`). Non-current retained revisions are not downloadable by users. Account deletion bypasses retention (`Due::Now`).

**Delete guard.** The desktop daemon holds a pair in `hold_mass_delete` or `root_missing` when the root is missing, unmounted (sentinel mismatch) or empty while the index is not, or when `desktop.mass_delete_threshold` (200) or more deletes are pending in either direction. Nothing is sent or applied until the user confirms. Remote deletes applied to the desktop go to the OS trash or recycle bin, and evicted placeholder files are never hydrated or deleted.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Version history (keep N revisions per file, user-visible restore) | Listed as out of scope in the acceptance criteria and restrictions; it needs a revision-list API plus restore UI in web, Android, iOS and desktop, and a quota rule for non-current bytes, whereas quota counts only current revisions of live files (data model A3, AC-74). The retention window gives the operator recovery with no new user-facing surface. |
| CRDT / version-vector merge | Files are opaque binaries (photos, office documents); no generic merge exists, and AC-8 forbids "a mix of both". A CRDT would add per-file merge metadata to every revision for a result that still ends in picking a winner. Real-time collaborative editing is out of scope. Named as the alternative family in `00_question_decomposition.md` and not selected. |
| User-facing trash with a restore button | Out of scope per the criteria; needs trashed-bytes quota accounting, a second retention clock and UI in four client families. R24 is instead mitigated by the OS trash on desktop and the delete guard, which is cheaper and covers the dominant failure (a sync root that vanished). |
| Client-clock last-writer-wins | Clock skew across devices could reorder commits; AC-8 explicitly requires server order, and per-account `epoch.seq` already provides a gapless commit order (data model 7.2). |

## Consequences

### Positive

- One rule decides every race: the per-account `seq`. Two concurrent commits serialise on the account feed row, so results are deterministic and testable (AC-69).
- No content is lost on a stale edit: the overwritten content stays beside the winner without copying bytes, and uses the same GC path as any replaced blob.
- Edit beats delete means a deleted-then-edited file never disappears under the editor; save-in-place by editors stays one revision on one file ID, so share links survive (AC-34).
- Retention rows are written in the commit transaction, so a crash cannot leave a replaced blob with no GC row, and backup restore stays consistent when retention exceeds the backup interval (F12 step 5, R33).
- The delete guard stops the worst case (unmounted drive deleting the cloud copy) with 0 deletions propagated until confirmed (AC-29).

### Negative

- The earlier of two offline edits is displaced into a conflict copy the user may never notice; validation scenario 3 records this as accepted data loss.
- A buggy client or ransomware on one device propagates changes to all devices; only the operator can recover, and only within retention. Retention 0 disables recovery.
- Replaced and deleted blobs hold disk for the retention window (up to 90 days at the maximum setting); the operator tunes it.
- Conflict-copy names can pile up in busy folders, and the Windows reserved-name marker and the ` (2)` collision suffix are still open owner items.
- The guard exists only in the desktop client; web, mobile and API deletes are not batch-guarded.

### Neutral / Open

- A device offline longer than the tombstone horizon (90 days) gets `resync_required` and performs the safe full resync (AC-41); this does not change the conflict rule.
- Restore uses the revision current at the requested time; if its blob is already collected the command reports "beyond retention" and changes nothing.
- No change to auth or storage layout; blob encryption (ADR-006) is orthogonal.

## Evidence

- `_docs/02_document/system-flows.md` § Flow F8: Conflict handling; § Flow F12: Deletion, blob GC, retention and operator restore
- `_docs/02_document/components/02_tree/description.md` § Conflict and delete rules realised here
- `_docs/02_document/components/14_desktop_daemon/description.md` § 5. Implementation Details (DeleteGuard); § 15. Risks
- `_docs/02_document/data_model.md` § 5.2 `revisions`; § 6.2 `blob_gc`; § 7.2 Rules for `epoch.seq`
- `_docs/02_document/canonical-names.md` § Technical defaults applied at Step 4 (restore re-points the revision, server-assigned conflict-copy name, desktop remote deletes to the OS trash)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R24, R26, R33)
- `_docs/00_research/raw/b_server_storage.md` (finding "No versions / no trash (restriction)")
- `_docs/00_research/05_validation_log.md` § Scenario 3: Concurrent offline edits (AC-8)
- `_docs/00_problem/restrictions.md` § Sync model; `_docs/02_document/tests/restriction-ids.md` (RESTRICT-10, RESTRICT-24)

## Notes

`architecture.md` § 8 lists this decision as "ADR-007" in its short index; the file numbering used by the Step 4.5 plan (005) is authoritative for this directory. Re-evaluate adding version history only if the owner reverses the "no version history" restriction; the revision rows and `blob_gc` queue already exist, so the cost would be API, quota and UI, not storage layout.
