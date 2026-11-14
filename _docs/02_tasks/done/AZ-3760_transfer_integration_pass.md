# Transfer integration pass with the real account guard and media effect

**Task**: AZ-3760_transfer_integration_pass
**Name**: Transfer integration pass
**Description**: Re-run the transfer account-state refusals with the real E-15 `AccountGuard` and the commit paths with the real E-18 media job effect injected, replacing the port-level test implementations
**Complexity**: 2 points
**Dependencies**: AZ-3705_tree_spike_names_effect_tests, AZ-3734_transfer_start_upload, AZ-3735_transfer_put_block, AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3757_transfer_erase_and_crash_cases, AZ-3690_account_admin_and_guard, AZ-3841_job_planner_enqueue_effect
**Component**: 03_transfer
**Tracker**: AZ-3760
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — effect protocol and the contract-test plan of E-13/c01.
- `_docs/02_document/contracts/03_transfer/commit_service.md` — effects list passed to `publish`.

## Problem

Until E-15 and E-18 exist, the transfer tests check the account state through a port-level test implementation and run commits without 08's media effect. A drift between 03's effects list and 08's real effect, or a refusal path that the real guard does not trigger, would first show in the pipelines. This task closes that gap at the component boundary (E-14 risk 4).

## Outcome

- Every transfer entry point refuses a deleting account through the real guard, with nothing written.
- Commits run with the real media effect in the same transaction: a job row exists only for committed entries.
- The contract-test cases the E-13/c01 plan assigns to E-14 pass.

## Scope

### Included
- IT-03-15's refusal lines and IT-03-1's `begin_deletion` line with the real `AccountGuard` and `AccountAdmin::begin_deletion`.
- `commit_upload` and `commit_batch` with the real media effect in the effects list.
- The E-14 rows of the contract-test plan in the commit contract.

### Excluded
- HTTP-level runs (pipelines AZ-3655, AZ-3656, AZ-3665).

## Acceptance Criteria

**AC-1: Real guard refuses every entry point**
Given an account moved to `deleting` by the real `AccountAdmin::begin_deletion`
When `start_upload`, `put_block`, `stage_small`, `commit_upload` and `commit_batch` are called for it
Then each returns `AccountNotActive`, and no session, staged row, node or revision is created

**AC-2: Media job in the commit transaction**
Given a committed JPEG through `commit_upload` with the real media effect, and a second JPEG whose session was removed before `publish`
When both commits run
Then a job row exists for the first after COMMIT and none for the second

**AC-3: Batch with media jobs and replay**
Given a batch with 3 images, one with an invalid name
When it is committed and then resent identically
Then job rows exist for the 2 committed images only, and the replay adds no job row

**AC-4: Plan cases pass**
Given the E-14 rows of the contract-test plan (E-13/c01)
When they run against 02's real `publish`, 03's real effects and 08's real effect
Then all pass

## Non-Functional Requirements

**Reliability**
- Same PostgreSQL 18 and temp blob root setup as the component suite.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (integration task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Account `deleting` (real 01) | Five entry points (IT-03-15, IT-03-1) | `AccountNotActive`, nothing created | NFT-SEC-27 |
| AC-2 | Two JPEG commits, one losing its session | Job rows | One job row | IT-03-9 |
| AC-3 | Batch of 3 images, 1 bad | Commit and replay (IT-03-8) | 2 job rows; replay adds none | AC-40 |
| AC-4 | Plan rows | Contract tests | All pass | IT-03-19 |

## Constraints

- No port of a sunnycloud component is replaced by a test implementation in this task.

## Risks & Mitigation

**Risk 1: Effects list order**
- *Risk*: the media effect runs before the blob effect and sees an entry that later fails.
- *Mitigation*: AC-2 checks that a failing entry leaves no job row.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AC-1 depends on the `AccountGuard` port being added to 03's ports (see E-14/c04) | E-14 owner, E-00 | resolved 2026-11-13 (owner, see AZ-3734's spec, Flagged concerns): 03 takes `Arc<dyn AccountGuard>` (`UploadPorts::guard`) like 02 | Medium |
| Account creation: 01 creates an account only inside its sign-in (live OIDC provider). The tests create it as AZ-3720 does: 01's `INSERT INTO accounts` text, then the real `initialize` of 07a, 05 and 02 in one system-scope transaction (`tests/transfer/real_account_repo.rs`, which needs a line in `scripts/ci/run-system-allowlist.txt`); 03's own `initialize` writes nothing and is not run | E-14/c15 (AZ-3760) | Implemented as described | Low |
| The media effect runs after 03's effects in the slot, so a 03 entry failure stops the entry before 08 plans a job; the case where 08's effect ran and a later effect of the same entry fails is covered by 02's AZ-3720 (`real_effects.rs`, with an injected fault effect) | E-14/c15 (AZ-3760) | Noted | Low |
