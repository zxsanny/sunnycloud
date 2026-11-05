# Tree mutation and conflict pipeline (F8, F12 step 1)

**Task**: AZ-3659_pipeline_tree_conflicts
**Name**: Tree mutation and conflict pipeline
**Description**: Folder create, rename, move and delete routes and stale-base commits apply the conflict rules, quota and GC effects and feed stamps, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3800_status_assignment_spike, AZ-3801_crate_skeleton_accept_loop, AZ-3829_tree_transfer_feed_handlers, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3708_tree_folder_mutations, AZ-3710_tree_conflict_rules, AZ-3711_tree_quota_gc_deltas, AZ-3713_tree_move_vs_edit, AZ-3750_quota_ledger, AZ-3752_quota_recompute, AZ-3753_quota_concurrency_suite, AZ-3729_blob_registry_gc_queue, AZ-3775_feed_stamp, AZ-3754_transfer_commit_batch, AZ-3655_pipeline_large_upload, AZ-3656_pipeline_small_upload_batch, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3659
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p07

## Problem

Conflict handling runs on two entry points: the commit path (03 into 02's `publish`) and the node routes (11 into 02's `TreeService`). Deletes also release quota (07a) and queue blobs for GC (04), and both entry points stamp the feed (05). The races in AC-70 and AC-74 happen between these entry points, which only meet in the running server. No planned task drives them together through the real routes.

## Outcome

- Stale edits, edit-versus-delete, name collisions, moves and deletes behave per F8 through the real `serve` process, and each change reaches the feed.
- Deletes release quota and queue the blob for GC in the same transaction.
- Every component operation of these mutations has a production caller; at least one integration test runs them through the real binary over HTTP.

## Scope

### Included
- Flow order and seams (`system-flows.md` F8 steps 1-6, F12 step 1):
  1. Node routes → `TreeService::{create_folder, update_node, delete_node}` (delete carries the base revision) → `QuotaLedger::adjust_used` and `GcQueue` schedule on delete → `FeedStamp::stamp` last.
  2. Commit routes (E-22/p03, p04) with a base revision → `TreeCommit::publish` → conflict copy builder, edit-beats-delete revival, new-name collision rename → `FeedStamp::stamp` for both nodes.
  3. 11 fills `CommitActor.device_label` from the principal so conflict-copy names carry the committing device.
- The end-to-end integration test of the steps above.

### Excluded
- GC sweeping and restore (E-22/p11), conflict handling on desktop (E-24/p03), wire status choice 409 vs 412 (E-21/c02).

## Acceptance Criteria

AC-1..AC-5 are local to this task. AC-70 and AC-74 keep their numbers from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Stale edit becomes a conflict copy**
Given file x at revision R2 and a device whose base is R1
When that device commits a new version of x
Then the commit is current, the superseded R2 sits in a new sibling node named `x (conflict copy YYYY-MM-DD HH-MM <device label>)` sharing the same blob, and both nodes appear in the feed

**AC-2: Edit beats delete; stale delete is refused**
Given a tombstoned file and, separately, a file edited since a device's base
When a device commits an edit of the tombstoned file, and another device deletes the edited file with its stale base
Then the tombstoned file is revived with no conflict copy, the stale delete is refused with `edited_since`, and a repeated delete of a deleted file succeeds idempotently

**AC-3: Name rules on both entry points**
Given a folder holding `A.txt`
When a new file `a.txt` is committed into it, a folder is renamed onto `A.txt`, a case-only rename is applied, and a folder is moved into its own subtree
Then the new file is stored under the server's conflict-copy name, the rename gets `name_conflict`, the case-only rename keeps the node id, and the move gets a cycle error

**AC-4: Delete effects in the same transaction**
Given a committed file
When it is deleted with its current base
Then the feed shows a tombstone, the account usage drops by the file size, and a GC row exists due at now plus retention

**AC-5: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `TreeService::{create_folder, update_node, delete_node}`, the conflict path of `TreeCommit::publish`, `QuotaLedger::adjust_used` and the `GcQueue` schedule call from the delete path, and `FeedStamp::stamp` from every mutation

### Interaction criteria (Step 1.8)

**AC-70: Move versus edit across devices** (R3)
Given folder F containing file x and two devices A and B with current bases
When A moves F into folder G while B commits an edit of x at the same moment (20 rounds)
Then every round ends with x inside the moved folder holding B's content, 0 conflict copies, the same file id, and both changes in the feed

**AC-74: Usage stays exact under concurrent commits and deletes** (R7)
Given an account with quota Q
When 1,000 operations run concurrently through the real routes (batch commits of new files, edits and deletes)
Then the stored usage equals the sum of the sizes of current revisions of live files (the quota drift check reports 0), usage never exceeds Q at any sampled point, and refused operations get 507 `quota_exceeded`

## Non-Functional Requirements

**Reliability**
- Server order decides "later"; client clocks are ignored (AC-8).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Static caller check over non-test sources | Each named operation has a production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | x at R2, device base R1 | Commit | Conflict copy sibling with device label, both in feed | AC-8 |
| AC-2 | Tombstoned file; edited file | Edit; stale delete; repeat delete | Revived; `edited_since`; idempotent | AC-8 |
| AC-3 | Folder with `A.txt` | New `a.txt`, rename onto, case-only rename, cycle move | Server name; `name_conflict`; same id; cycle error | AC-42 |
| AC-4 | Committed file | Delete | Tombstone, usage drop, GC row | AC-30 |
| AC-70 | F with x, devices A, B | Concurrent move and edit, 20 rounds | x in moved folder, 0 copies, same id | FT-N-42 |
| AC-74 | Quota Q | 1,000 concurrent commits, edits, deletes | Usage = live sum, never above Q, drift 0 | NFT-RES-38 |

## Constraints

- Owner: component 12 (`src/server/`): bindings for these routes and effects, and the flow's end-to-end tests among 12's integration tests. No SQL or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume over HTTP; no sunnycloud component is faked.
- Component suites keep their single-boundary parts: AC-70 locking in E-13/c09, AC-74 ledger concurrency in E-11/c05.
- Downstream: E-31/c24 (FT-P-10..12, FT-P-15), E-31/c31 (FT-N-04, FT-N-05), E-31/c33 (FT-N-42) and E-31/c41 (NFT-RES-38) should depend on this task.

## Risks & Mitigation

**Risk 1: Delete path skips the quota port**
- *Risk*: the delete route reaches 02 without the real quota ledger bound, so usage drifts only after deletes.
- *Mitigation*: AC-4 and AC-74 check usage after real deletes.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 409 vs 412 for `edited_since` is still a spike (E-21/c02, G-11-1) | E-21 | open | Low |
| NFC/NFD stored form (G-1) is an owner item (E-13/c01) and changes AC-3's case matrix | Owner via E-13 spike | open | Low |
