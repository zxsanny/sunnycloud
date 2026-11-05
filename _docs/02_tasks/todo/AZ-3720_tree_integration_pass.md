# Tree integration pass with real transfer, identity and media counterparts

**Task**: AZ-3720_tree_integration_pass
**Name**: Tree integration pass
**Description**: Re-run the tree's guard, effect-order, no-dangling-blob and erase cases with the real E-15 `AccountGuard`, the real E-14 commit effects and the real E-18 media effect and thumbnail source in place of port-level test implementations
**Complexity**: 2 points
**Dependencies**: AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3717_tree_lifecycle_blob_cleanup, AZ-3718_tree_thumb_state_listing, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3690_account_admin_and_guard, AZ-3841_job_planner_enqueue_effect, AZ-3844_thumbnail_states_derived_opener
**Component**: 02_tree
**Tracker**: AZ-3720
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — the effect protocol and the contract-test plan from E-13/c01.
- `_docs/02_document/contracts/03_transfer/commit_service.md` — the real 03 effects used here.

## Problem

Until E-14, E-15 and E-18 exist, the tree tests use port-level test implementations of `AccountGuard`, `CommitEffect` and `ThumbnailStateSource`. Those prove the tree's side only. The "no fake stores" cases IT-02-10, IT-02-12 and IT-02-23 must run again against the real implementations, so a drift in the effect contract (E-13 risk 2) shows up at the component boundary and not first in the pipelines.

## Outcome

- The guard, statement order, dangling-blob invariant and erase cases pass with the real counterparts.
- The contract-test cases assigned to E-13 by the E-13/c01 plan pass.

## Scope

### Included
- IT-02-10 with the real `AccountGuard` (account moved to `deleting` by the real `AccountAdmin::begin_deletion`) and the real 03 and 08 effects.
- IT-02-12 with the real `BlobCommitEffect`.
- IT-02-23 with accounts created and marked deleting through the real 01 operations.
- One listing with the real `ThumbnailStateSource`.

### Excluded
- HTTP-level runs (pipelines AZ-3655, AZ-3656, AZ-3665).

## Acceptance Criteria

**AC-1: Real guard on every entry point**
Given an account moved to `deleting` by the real `AccountAdmin::begin_deletion`
When `publish`, `create_folder`, `update_node` and `delete_node` are called for it
Then each returns `AccountNotActive` with 0 rows, no quota change and no seq consumed

**AC-2: Real effects keep the order (AC-69)**
Given a batch published with the real `ReceiptEffect`, `BlobCommitEffect` and media effect, statement log on
When the batch commits
Then every effect statement precedes the stamp, 0 statements run between stamp and COMMIT, and a repeated identical batch returns `Replayed`

**AC-3: No dangling revision with the real blob effect (AC-68)**
Given 300 publishes through the real `BlobCommitEffect` with injected `Entry` and `Batch` failures (probability 0.3)
When the invariant probe runs after each
Then 0 committed revisions reference a missing or non-committed blob

**AC-4: Media job follows its entry**
Given a batch with a JPEG entry and a JPEG entry whose savepoint fails
When it is published with the real media effect
Then a job row exists for the first entry and none for the failed one

**AC-5: Erase with real identity operations (AC-27)**
Given an account created through the real 01 account creation (root via `initialize`) with 1,000 nodes and 600 revisions, then moved to `deleting`
When the tree's `erase_batch` loop runs with the real sweeper
Then the IT-02-23 invariants hold and `remaining` reaches 0

**AC-6: Real thumbnail states**
Given images with derived thumbnails in the real 08 store
When their folder is listed
Then the real source is called once per page and ready thumbnails show `ready`

## Non-Functional Requirements

**Reliability**
- The run uses the same PostgreSQL 18 and temp blob root setup as the component suite.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (integration task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Account `deleting` (real 01) | Four entry points (IT-02-10) | `AccountNotActive`, nothing changed | NFT-SEC-27 |
| AC-2 | Real 03 and 08 effects | Statement log, replay (IT-02-10) | Order kept; `Replayed` | NFT-RES-35 |
| AC-3 | Real blob effect | Invariant probe x300 (IT-02-12) | 0 dangling | NFT-RES-34 |
| AC-4 | Two JPEG entries, one failing | Job rows | One job row | |
| AC-5 | Real account lifecycle | Erase loop (IT-02-23) | Invariants; remaining 0 | NFT-RES-30 |
| AC-6 | Real derived thumbnails | Listing (IT-02-16 part) | One call per page; `ready` | NFT-PERF-10 |

## Constraints

- No port of a sunnycloud component is replaced by a test implementation in this task.

## Risks & Mitigation

**Risk 1: Contract drift found late**
- *Risk*: a real effect issues a statement after the stamp or reads beyond its rows.
- *Mitigation*: AC-2's statement log; a failure goes back to the effect's owner task.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | — | — |
