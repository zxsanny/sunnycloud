# Hold-cap error and purge-horizon rule (spike, G-05-2 and G-05-3)

**Task**: AZ-3773_feed_gaps_spike
**Name**: Feed gaps spike
**Description**: Decide the typed error and wire code for a principal's fifth concurrent long-poll, and whether `raise_purge_horizon` may lower the horizon, and record both so IT-05-6 and IT-05-12 can be written
**Complexity**: 2 points
**Dependencies**: None
**Component**: 05 sync_feed
**Tracker**: AZ-3773
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c01

## Problem

Two rules of the sync feed are open and marked "resolve during decompose" in `tests/spec-gaps.md`: G-05-2, the typed error when a principal exceeds `max_holds_per_principal` (4), which IT-05-6 and ST-05-3 assert by name; and G-05-3, whether `FeedHorizon::raise_purge_horizon` is a plain update (it can move the horizon down) or never decreases, which blocks IT-05-12. Without them the long-poll and horizon tasks cannot state their acceptance criteria.

## Outcome

- G-05-2 decided: the error variant name, its wire code and HTTP status, and whether a `Retry-After` is sent. Recorded in the sync feed component description (ChangeFeed errors row), in `tests.md` (IT-05-6, ST-05-3), and as a proposed row for the error helper's status table, with a note to the E-02 and E-21 owners.
- G-05-3 decided: the horizon either never decreases or follows the plain update. Recorded in the description (FeedHorizon section) and in IT-05-12, whose BLOCKED marker is removed.
- `tests/spec-gaps.md` moves both gaps to section 1 (resolved) with the date. This unblocks E-12/c05 (hold cap) and E-12/c06 (horizon).

## Scope

### Included
- The two decisions with the epic owner; updates to the 05 description, its `tests.md`, `tests/spec-gaps.md`; notes to the E-02 and E-21 owners and to the E-13 owner (the purge caller) if the horizon rule changes its obligations.

### Excluded
- Implementing either rule (E-12/c05, E-12/c06); changing the error helper's table or the HTTP mapper (E-02, E-21).

## Acceptance Criteria

**AC-1: Hold-cap error decided**
Given G-05-2
When the decision is recorded
Then the 05 description names the error variant, wire code and HTTP status for the fifth concurrent hold of one principal, IT-05-6 and ST-05-3 assert that code, and a proposed status-table row is noted for the E-02 owner

**AC-2: Horizon rule decided**
Given G-05-3
When the decision is recorded
Then the description states whether a call with a lower `seq` leaves the horizon unchanged, and IT-05-12 has a concrete expected result with no BLOCKED marker

**AC-3: Gaps closed**
Given both decisions
When `tests/spec-gaps.md` is updated
Then G-05-2 and G-05-3 appear in section 1 (resolved) with the date and the affected tests

## Non-Functional Requirements

**Compatibility**
- A new wire code is additive for clients (unknown codes are treated as `internal`, helper 02 rules).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (decision task) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Updated description and `tests.md` | Document review | Variant, code, status named; IT-05-6 and ST-05-3 updated | |
| AC-2 | Updated description and `tests.md` | Document review | Rule stated; IT-05-12 unblocked | |
| AC-3 | `tests/spec-gaps.md` | Document review | Both gaps resolved | |

## Constraints

- Decision only; any change to the shared error table or HTTP mapping is done by its owner (E-02, E-21).

## Risks & Mitigation

**Risk 1: New wire code without a status-table row**
- *Risk*: 11 maps the new error to 500.
- *Mitigation*: AC-1 requires the proposed row and the note to the E-02 and E-21 owners.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| If the hold-cap code is new, the error helper's status table, 11's mapper and the OpenAPI document need it | E-02 / E-21 | resolved 2026-11-07: no new code, `rate_limited` (429) is reused | Low |

## Resolution (2026-11-07)

Owner-approved decisions, recorded by the document workers:
- G-05-2: the 5th concurrent long-poll of a principal gets the new typed variant `ChangeFeedError::TooManyHolds{retry_after}` (additive, proposed in `contracts/part2_tree_transfer_feed.md`), sent as the existing `rate_limited` (HTTP 429) with `Retry-After` (5 s proposed, not agreed). The new-code route in Outcome and Risk is replaced by reuse of `rate_limited`: no new status-table row, no new OpenAPI code.
- G-05-3: `raise_purge_horizon` is non-decreasing (`GREATEST`); a lower seq leaves the horizon unchanged and is not an error, so purge batches that commit out of seq order are safe (AC-3 of the purge janitors, AZ-3714).
- Recorded in: `components/05_sync_feed/description.md` (hold cap, FeedHorizon), `components/05_sync_feed/tests.md` (IT-05-6, ST-05-3, IT-05-12), `components/11_http_api/description.md`, `contracts/11_http_api/problem_details.md`, and the flagged concerns of AZ-3658, AZ-3714, AZ-3810 and AZ-3829.
