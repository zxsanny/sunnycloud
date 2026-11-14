# Move-versus-edit race and the AC-70 suite

**Task**: AZ-3713_tree_move_vs_edit
**Name**: Tree move-versus-edit race
**Description**: Prove at the tree boundary that a folder move and an edit of a file inside it serialise on the account lock and that the edit, addressed by node id, always lands in the moved folder with no conflict copy
**Complexity**: 3 points
**Dependencies**: AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3711_tree_quota_gc_deltas, AZ-3776_change_feed_read
**Component**: 02_tree
**Tracker**: AZ-3713
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — commit by node id.
- `_docs/02_document/contracts/02_tree/tree_service.md` — move semantics.

## Problem

Interaction risk R3: device A moves a folder while device B saves a file inside it. If the edit were resolved by path, it would land in the old place, be lost or become a conflict copy. AC-70 requires the edit inside the moved folder, 0 conflict copies and an unchanged file id. The pipeline AZ-3659 checks the cross-component outcome through the routes; the locking mechanism belongs here (`interaction-risks.md`, R3 row).

## Outcome

- `update_node` (move) and `publish` with `Existing` on a file inside the folder serialise on the account advisory lock in either order.
- In every iteration the file keeps its id and parent, the folder sits in its new parent, the new revision is current, and there is no conflict copy.

## Scope

### Included
- The race suite IT-02-14 at the `TreeService`/`TreeCommit` boundary, both orders forced and random skew.
- Any lock-ordering or lookup fix inside the tree that the suite shows is needed (the edit must address the node by id; the move must hold the lock before its cycle check).
- Statement-log check that a move's cycle check precedes its stamp.

### Excluded
- The HTTP-level run with two devices (pipeline AZ-3659, AC-70).
- Commit-side use of `parent_id` as a hint only (E-14/c12).

## Acceptance Criteria

AC-1 is local to this task. AC-70 keeps its number from `acceptance_criteria.md`.

**AC-70: Move versus edit (R3, component part)**
Given folder F containing file f at revision r1
When `update_node(F -> G)` and `publish(Existing{f, base = r1})` start at a barrier on two connections, for 500 iterations with random start skew of 0 to 5 ms and each order forced in 50 of them
Then in every iteration f's node id is unchanged, f's parent is F, F's parent is G, f's current revision is the new one, there are 0 conflict copies, the usage delta is applied once, and both changes are in the feed

**AC-1: Stamp stays last in a move**
Given the statement log of a move whose cycle check walks 30 levels
When the move commits
Then every cycle-check statement precedes the stamp and nothing follows the stamp before COMMIT

## Non-Functional Requirements

**Performance**
- The suite finishes within 60 s on the component runner.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None beyond the integration suite | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-70 | F with f at r1 | 500 barrier iterations, both orders forced (IT-02-14) | Same id, f under moved F, 0 copies, one delta, both in feed | FT-N-42, AC-34 |
| AC-1 | 30-level move | Statement log | Cycle check before stamp | AC-69 |

## Constraints

- Real 05 and 07a on one PostgreSQL; several real connections, not transactions, give the isolation.

## Risks & Mitigation

**Risk 1: Flaky timing**
- *Risk*: random skew hides one order.
- *Mitigation*: 50 iterations per forced order in addition to the random ones.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | — | — |
