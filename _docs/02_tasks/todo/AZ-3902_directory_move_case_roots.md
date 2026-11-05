# Directory moves, unstable-id fallback and case-insensitive roots

**Task**: AZ-3902_directory_move_case_roots
**Name**: Directory moves and case rules
**Description**: Recognise a renamed or moved directory as one `move_node` of the directory node (children not re-sent), match vanished and new directories by their files' `(size, mtime, hash)` multiset where directory ids are unstable, and on case-insensitive roots treat a case-only change as a rename while case-colliding names on case-sensitive roots lose neither file
**Complexity**: 3 points
**Dependencies**: AZ-3901_decide_from_disk_pair_worker, AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3903_delete_guard_holds, AZ-3767_sc_names
**Component**: 14 desktop_daemon
**Tracker**: AZ-3902
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c10b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_names/sc_names.md` — `name_key` (E-05/c01).

## Problem

Renaming a folder of 2,000 files must not become 2,000 deletes and 2,000 uploads: that would trip the mass-delete hold and re-send every byte, because there is no cross-node dedupe (R26). Some file systems (some network and FAT volumes) have no stable directory ids, so the daemon needs a safe fallback. On case-insensitive roots (NTFS, ext4 casefold, macOS later), `Report.txt` to `report.txt` is a rename, and two paths that differ only in case cannot coexist (AC-42); on case-sensitive Linux roots, `A.txt` and `a.txt` must both survive (FT-N-05).

This is the second half of the split E-24/c10 row (see E-24/c10a).

## Outcome

- A directory whose `file_id` is known and whose old path is gone is one `move_node` of the directory node; children are not uploaded or deleted.
- Without stable directory ids, a vanished directory and a new one are matched by the multiset of `(size, mtime, hash)` of their files; only a full match moves; otherwise it falls back to create plus delete, and the deletes go through the delete guard.
- The `is_case_insensitive` result stored on the pair at `add_pair` drives path comparison by `name_key`; a case-only change is `move_node` with the new name on the same node.
- On a case-sensitive root, two local names that differ only in case are both uploaded; the server resolves the collision with a conflict copy, and neither file is lost.

## Scope

### Included
- Directory-move detection in the decide step of E-24/c10a.
- The multiset fallback and its full-match rule.
- Case-only rename on case-insensitive pairs; case collisions on case-sensitive pairs.

### Excluded
- The probe itself (E-24/c07, E-24/c08) and the index's `name_key` lookups (E-24/c06); the guard (E-24/c11).

## Acceptance Criteria

**AC-1: Directory rename is one move**
Given a synced directory `d` with 2,000 files on a root with stable ids
When it is renamed to `e`
Then exactly 1 `move_node` is sent, with 0 uploads and 0 deletes, and the node id of `e` equals the old node id of `d`

**AC-2: Unstable ids, full match**
Given the same rename where the fake `file_id` returns none for directories and the contents are identical
When the vanished and the new directory are compared
Then their `(size, mtime, hash)` multisets match and 1 `move_node` is sent

**AC-3: Unstable ids, partial match**
Given the rename with part of the content changed
When the multisets differ
Then the daemon falls back to create plus delete, the 2,000 deletes are counted individually by the delete guard, and the pair goes to `hold_mass_delete` with nothing deleted on the server

**AC-4: Case-only rename on case-insensitive roots (AC-42)**
Given a case-insensitive pair (ext4 casefold, NTFS on WIN-VM) with `Report.txt`
When it is renamed to `report.txt`
Then one `move_node` with the new name is sent on the same node id, and the index holds one entry, not two

**AC-5: Case collision on a case-sensitive root (FT-N-05)**
Given a case-sensitive ext4 root with local `A.txt` and `a.txt`
When both are synced
Then both files are uploaded, the server answers one with a conflict copy, and neither local file is lost

## Non-Functional Requirements

**Reliability**
- No fallback path deletes anything except through the delete guard.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Directory id known, old path gone (fake `PlatformFs`) | One `move_node`, 0 uploads, 0 deletes |
| AC-2, AC-3 | Multiset equal and unequal with `file_id` none | Move; create plus delete through the guard |
| AC-4 | Case-only change on a case-insensitive pair | `move_node` with new name; one entry |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-4 | ext4, ext4 casefold, NTFS on WIN-VM; directory with 2,000 files | IT-14-7 | As AC-1..AC-4; 2,000 deletes hold the pair | AC-42, FT-P-15, FT-P-08 |
| AC-5 | Case-sensitive root with `A.txt`, `a.txt` | IT-14-7, FT-N-05 | Both kept; one conflict copy | AC-42 |

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- `move_node` has no base revision; a move never conflicts with an edit (AC-70).

## Risks & Mitigation

**Risk 1: A false full match**
- *Risk*: two different directories with equal file multisets are treated as one move.
- *Mitigation*: only a full multiset match moves (14 §5 step 4); a wrong match yields a move, which deletes nothing.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| NFC versus NFD names (G-1) is an open owner item; it decides whether two such names collide | Owner | open | Low |
