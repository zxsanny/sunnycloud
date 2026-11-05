# Transfer spike: resume bound (G-03-1) and stale-base commit response (G-03-3)

**Task**: AZ-3731_transfer_spike_resend_stale_base
**Name**: Transfer resume-bound and stale-base spike
**Description**: Take the owner two decisions: how the 16 MiB worst-case resend meets AC-3's 10 MiB (limits or AC change), and what the commit answers for a stale-base and a taken-name commit
**Complexity**: 2 points
**Dependencies**: None
**Component**: 03_transfer
**Tracker**: AZ-3731
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c01

## Dependencies

### Document Dependencies
- `_docs/02_document/tests/spec-gaps.md` — rows G-03-1 and G-03-3.
- `_docs/02_document/contracts/03_transfer/commit_service.md` — commit response, draft until G-03-3 is decided.

## Problem

G-03-1: with 4 lanes of 2 MiB in flight and up to 8 MiB acknowledged but not yet checkpointed, a restart can force a resend of 16 MiB, while AC-3 promises at most 10 MiB. Either the lane or checkpoint limits change, or the owner changes AC-3. Until then IT-03-4 case B, AT-03-1 and the system scenarios FT-P-01, NFT-PERF-05 and NFT-RES-01 may fail legitimately.
G-03-3: `commit_upload` lists `EditedSince`, but 02's `publish` never fails a stale base; it returns `ConflictCopied`. `Committed{node_id, revision_id, conflict_copy?}` is also used for `CreatedAsCopy`, where the new file itself carries the copy name. The wire meaning of `conflict_copy` and whether `EditedSince` ever occurs on commit are not written down; clients (E-23, E-26) and 11 (E-21/c02) need them.

## Outcome

- An owner decision on G-03-1, with the numbers behind each option, recorded where tests and config read it.
- An owner decision on G-03-3, recorded in the commit contract, which then moves from draft to frozen.

## Scope

### Included
- Options for G-03-1 with their worst-case resend and their cost: fewer lanes, a smaller `checkpoint_bytes` or `checkpoint_interval`, or an AC-3 wording at the true bound; the throughput risk of each against PT-03-1 and the WAL cost noted in `data_model.md` §6.3.
- Options for G-03-3: the `Committed` fields for each `PublishOutcome` (`Updated`/`Replaced`, `ConflictCopied`, `CreatedAsCopy`, `Revived`, `NoChange`) and whether `EditedSince` stays in the commit error list.
- Recording the decisions and informing E-21/c02 (409 vs 412), E-23 and E-26 owners and pipeline AZ-3655.

### Excluded
- Changing config, tests or the AC text (done by E-14/c03, E-14/c06 and the owner).

## Acceptance Criteria

**AC-1: G-03-1 options quantified**
Given the defaults (4 lanes, `checkpoint_bytes` 8 MiB, `checkpoint_interval` 1 s)
When the options are written up
Then each option states its worst-case resend in MiB and its effect on PT-03-1 throughput and checkpoint write volume

**AC-2: G-03-1 decision recorded (AC-3)**
Given the owner's answer
When it is recorded
Then the G-03-1 row in `tests/spec-gaps.md` states the decision and date, and either lists the new config defaults for E-14/c03 and the assertion bound for E-14/c06, or names the owner's new AC-3 text; if no answer comes, the row stays `open` and E-14/c06 keeps the 10 MiB assertion with the documented legitimate failure of case B

**AC-3: G-03-3 decision in the commit contract**
Given the owner's answer
When it is recorded
Then the commit contract states, for each publish outcome, the `Committed` fields returned (which node `node_id` and `conflict_copy` name in the stale-base and taken-name cases) and whether `EditedSince` can be returned by `commit_upload`, and its status is `frozen`

**AC-4: Downstream informed**
Given both decisions
When they are recorded
Then the G-03-3 row in `tests/spec-gaps.md` points at the contract, and the flagged concerns on E-21/c02 and pipeline AZ-3655 are updated by their owners

## Non-Functional Requirements

**Performance**
- Any option that lowers lanes or checkpoint size names the throughput gate it must pass (PT-03-1, AC-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Current defaults | Review of the options table | Worst case and cost per option | PT-03-1 |
| AC-2 | Owner answer | G-03-1 row | Decision, date, config or AC text | IT-03-4, AT-03-1, FT-P-01, NFT-PERF-05, NFT-RES-01 |
| AC-3 | Owner answer | Commit contract | Fields per outcome; `frozen` | IT-03-7 |
| AC-4 | Both decisions | G-03-3 row, linked concerns | Pointers updated | |

## Constraints

- Both answers are the owner's; the spike prepares options and records the result.

## Risks & Mitigation

**Risk 1: Lower limits cost throughput**
- *Risk*: fewer lanes or more frequent checkpoints miss AC-2.
- *Mitigation*: AC-1 makes the cost explicit before the owner chooses.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-03-1: worst-case resend 16 MiB against AC-3's 10 MiB; affects AC-3 for every client | Owner | open | Medium |
| G-03-3: commit response for stale-base and taken-name commits, and `EditedSince` on commit | Owner | open | Medium |
