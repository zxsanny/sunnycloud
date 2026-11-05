# Sync core spike: client consequences of G-03-1 (lanes and checkpoints vs AC-3)

**Task**: AZ-3953_sync_resend_bound_spike
**Name**: Client side of the resume bound
**Description**: Record how the owner's G-03-1 decision (taken through E-14/c01) sets the client's lane count, the resume assertion bound and the test expectations of the sync core
**Complexity**: 2 points
**Dependencies**: AZ-3731_transfer_spike_resend_stale_base
**Component**: 13_sync_core
**Tracker**: AZ-3953
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/tests/spec-gaps.md` — row G-03-1.
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `UploadPlanner` inputs (`lanes_max`).

## Problem

After a drop the worst case re-send is 16 MiB: 4 lanes of 2 MiB in flight plus up to 8 MiB acknowledged by the server but not yet checkpointed. AC-3 promises at most 10 MiB. Either the lane or checkpoint limits change, or the owner changes AC-3. This is an owner decision (G-03-1), prepared on the server side by E-14/c01. The client side owns the lane count it uses, the planner inputs and the AC-3 assertions in IT-13-2, AT-13-1, FT-P-01, NFT-PERF-05 and NFT-RES-01; they must follow the decision and must not decide it.

## Outcome

- The client's lane count and any client-side limit implied by the decision, recorded in the `sync_api` contract (`SyncConfig`) and 13 §3.
- The assertion bound for AT-13-1 and AC-3 tests on the client side, recorded in the G-03-1 row.
- If no owner answer exists, the row stays `open` and the client tests keep the 10 MiB assertion with the documented legitimate failure.

## Scope

### Included
- Reading the owner's G-03-1 answer from E-14/c01 and translating it into client values: effective lanes, whether the planner needs another input (for example a smaller in-flight cap), and the re-send bound to assert.
- Recording the values and naming the tasks that apply them (E-23/c05 planner inputs, E-23/c09b executor, E-23/c18b AT-13-1).

### Excluded
- The decision itself (owner, through E-14/c01).
- Server config defaults (E-14/c03) and server tests (E-14/c06).

## Acceptance Criteria

**AC-1: Client values follow the decision**
Given the owner's recorded G-03-1 answer
When the client consequences are written
Then the record states the client lane count, any extra planner or executor limit, and the worst-case re-send in MiB that results from the server and client values together

**AC-2: Assertion bound recorded (AC-3)**
Given the client values
When they are recorded
Then the G-03-1 row of `tests/spec-gaps.md` names the client assertion bound for AT-13-1 and the implementing tasks (E-23/c05, E-23/c09b, E-23/c18b)

**AC-3: No answer leaves the gap visible**
Given no owner answer by the time the spike closes
When it closes
Then the row stays `open`, the client tests keep the 10 MiB assertion, and the expected legitimate failure is noted next to AT-13-1

## Non-Functional Requirements

**Performance**
- Any lower lane count names the throughput gate it must still pass (PT-13-1, AC-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision record) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Owner answer | Client values record | Lanes, limits, worst case stated | AC-3 |
| AC-2 | Client values | G-03-1 row | Bound and tasks named | IT-13-2, AT-13-1, FT-P-01, NFT-PERF-05, NFT-RES-01 |
| AC-3 | No answer | G-03-1 row | Stays open; 10 MiB kept | |

## Constraints

- The decision is the owner's; this task only records its client consequences.

## Risks & Mitigation

**Risk 1: Fewer lanes miss AC-2**
- *Risk*: a lower lane count to meet AC-3 costs throughput.
- *Mitigation*: AC-1 states the worst case and the performance gate together.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-03-1: worst-case re-send 16 MiB against AC-3's 10 MiB; affects the client lanes and checkpoints and every client's AC-3 test | Owner (via E-14/c01) | open | Medium |
