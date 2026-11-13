# Conflict copies, new-name collisions and edit-beats-delete revival

**Task**: AZ-3710_tree_conflict_rules
**Name**: Tree conflict rules
**Description**: Add the `ConflictCopied`, `CreatedAsCopy` and `Revived` outcomes to `publish`: stale bases keep the overwritten revision in a conflict copy, taken names get the server-assigned conflict-copy name, and an edit revives a deleted file and its deleted ancestors
**Complexity**: 5 points
**Dependencies**: AZ-3709_tree_publish_core, AZ-3714_tree_cascade_purge_janitors, AZ-3765_sc_clock, AZ-3767_sc_names, AZ-3776_change_feed_read
**Component**: 02_tree
**Tracker**: AZ-3710
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — commit contract (produced by E-13/c05); this task adds the three outcomes.

## Problem

RESTRICT-24 and AC-8 say: the later commit by server order wins, a change based on a stale version keeps the overwritten content beside it as a conflict copy, and an edit beats a delete. A new file whose name is already taken must not fail or make the client retry: the server names it. These rules must hold inside the batch savepoints of `publish`, so a unique-index collision never aborts the transaction.

## Outcome

- A stale base makes the new revision current and moves the previous current revision row, with its blob and no byte copy, to a new sibling node with the conflict-copy name.
- A taken name on a new file answers `CreatedAsCopy` with a distinct server-assigned name; `name_conflict` never occurs on commit.
- A commit on a deleted file revives it and its deleted ancestors up to the first live one, without a conflict copy.

## Scope

### Included
- `ConflictCopied{copy_node}` for `Existing` with a stale `base_revision`: re-parent the superseded revision row to the new sibling; both nodes touched.
- `CreatedAsCopy{node}` for `New` whose name is taken.
- `Revived` for a commit on a tombstoned file: revive the node and its tombstoned ancestors, clear `cascade_pending` on each revived ancestor, keep descendants already marked by the cascade deleted, pass every revived node to the stamp; a name taken meanwhile gives a conflict-copy name.
- Names come from helper 06's `conflict_copy_name` (pattern, UTC from the injected clock in `conflict_copy_time_zone`, `CommitActor.device_label`, 255-byte truncation, ` (2)` suffix) with a folder name lookup supplied by the tree; a lost race on the unique index inside the savepoint retries with the next free name.

### Excluded
- Quota deltas of these outcomes (E-13/c07); the cascade itself (E-13/c10).
- Building the name text (helper 06, E-05/c01): this task never re-implements it.

## Acceptance Criteria

**AC-1: Stale base becomes a conflict copy (AC-8)**
Given node N with r1, device D1 has published r2 on base r1, and the clock reads 2026-11-04 09:07 UTC
When device `phone-2` publishes r3 on base r1
Then N's current revision is r3, a sibling named `<stem> (conflict copy 2026-11-04 09-07 phone-2).<ext>` holds the r2 revision row (same revision id and blob id), the revision count grew only by r3, the outcome is `ConflictCopied{copy_node}`, and both nodes appear in the feed at the same seq

**AC-2: Collision suffix and long stems**
Given the conflict-copy name from AC-1 is already taken, and separately a file with a 250-byte stem
When another stale-base publish happens for each
Then the first copy gets a numeric suffix, and the second copy's full name is at most 255 bytes

**AC-3: New file whose name is taken (AC-8, AC-42)**
Given `A.txt` exists in the root
When `New{root, "a.txt"}` is published, and two more `New` entries for the same name are published concurrently
Then each loser gets `CreatedAsCopy{node}` with distinct names, `A.txt` is untouched, no `name_conflict` code appears, and no transaction is aborted

**AC-4: Edit beats delete with ancestor revival (AC-8)**
Given `R/A/B/f` and `R/A/B/g`, and A deleted with the cascade run so B, f and g are marked
When `Existing{f, base = f.current}` is published
Then f, B and A are live, `cascade_pending` is cleared on A and B, g stays deleted, no conflict copy exists, and the change feed shows A and B from tombstone to live plus f's revision, all at one seq

**AC-5: Revived name taken meanwhile**
Given the AC-4 setup and a new live `f` created in B's place after the delete
When the edit of the old f is published
Then the revived f gets a conflict-copy name and the new `f` is untouched

## Non-Functional Requirements

**Reliability**
- Time comes only from the injectable clock; client clocks are ignored.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Folder name lookup used by the helper | Taken names reported; suffix chosen by the helper |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | N with r1, r2; manual clock | Stale publish by `phone-2` (IT-02-6) | Copy holds r2 row; one seq | FT-P-10, AT-02-1 |
| AC-2 | Name taken; 250-byte stem | Stale publish (IT-02-6) | Suffix; name <= 255 bytes | |
| AC-3 | `A.txt` exists | `a.txt` + 2 concurrent (IT-02-7) | `CreatedAsCopy`, distinct names, no abort | FT-P-15 |
| AC-4 | A deleted and cascaded | Edit f (IT-02-8) | f, B, A live; g deleted; one seq | FT-P-11 |
| AC-5 | Name re-taken | Edit old f (IT-02-8 variant) | Conflict-copy name | FT-P-12 |

## Constraints

- AC-4 needs the cascade to have marked descendants, hence the dependency on E-13/c10.
- The pipeline AZ-3659 owns the end-to-end version through the routes; this task keeps the `publish` boundary.

## Risks & Mitigation

**Risk 1: Unique violation aborting a batch**
- *Risk*: a collision outside a savepoint aborts all 500 entries.
- *Mitigation*: every name write happens inside the entry savepoint; AC-3 runs concurrent collisions.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Reviving an ancestor before the cascade has marked its descendants clears `cascade_pending`, so unmarked siblings (g in AC-4) come back live; 02 §2 and `data_model.md` §5.1 only say marked descendants stay deleted | E-13 owner / Owner | resolved 2026-11-13 (owner): reviving an ancestor clears its `cascade_pending`, so descendants the cascade has not marked yet come back live; marked ones stay deleted. Recorded in `data_model.md` §5.1 | Medium |
| `sync_conflict_copies_total` (observability.md, Sync table) has no owning task in E-13; this task creates the copies | E-09 / E-13 owner | open | Low |
