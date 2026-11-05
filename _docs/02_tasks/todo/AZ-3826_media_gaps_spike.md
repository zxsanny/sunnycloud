# Media gaps: thumbnail-states shape, Range on input, obsolete rule, hash encoding (spike)

**Task**: AZ-3826_media_gaps_spike
**Name**: Media decompose-time gaps
**Description**: Record the `thumbnail-states` answer for foreign ids and over-long bodies (G-08-3), whether the worker input route honours `Range` (G-08-4), when a queued job's revision counts as obsolete, and the wire encoding of `content_hash` in `ClaimedJob`
**Complexity**: 2 points
**Dependencies**: None
**Component**: 08 media_jobs
**Tracker**: AZ-3826
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c01

## Problem

Four 08 behaviours are unspecified or contradictory, and tests or the worker protocol need one answer each:
- G-08-3: what `thumbnail-states` returns for ids of another account, and whether a body over 1,000 ids is 400 or 413 (ST-08-4, AT-08-1).
- G-08-4: whether `GET input` on the worker listener supports `Range` or ignores it (IT-08-12).
- Obsolete rule: 08 §4, IT-08-12 and the tree contract serve a superseded or deleted revision while it is retained, but FT-N-49, `data_model.md` §9.1 (outcome `obsolete`), flow F10 and 09's integration tests expect a job for a superseded or deleted revision to end `obsolete`.
- The JSON encoding (hex or base64) of `content_hash` in `ClaimedJob` (media-shared skeleton contract open point 1).

## Outcome

- One recorded answer per question in `components/08_media_jobs/description.md` and `tests/spec-gaps.md`, with the date.
- The answers unblock E-18/c02 (worker job protocol), E-18/c07 (input route), E-18/c09 (thumbnail states) and E-18/c13 (ST-08-4), and E-20/c03 (supervisor client).

## Scope

### Included
- The four answers, the test expectations they change, and owner sign-off where an acceptance criterion changes.

### Excluded
- Implementing them (E-18/c02, c07, c09).

## Acceptance Criteria

**AC-1: thumbnail-states recorded (G-08-3)**
Given ST-08-4 and AT-08-1
When the spike ends
Then 08 §5 states the answer for foreign or unknown ids (absent or `None`) and one status for a body over 1,000 ids, ST-08-4 uses them, and `tests/spec-gaps.md` shows G-08-3 resolved

**AC-2: Range on input recorded (G-08-4)**
Given IT-08-12
When the spike ends
Then 08 §5 states whether `Range` on the worker input route is honoured or ignored, and IT-08-12 has one expected result

**AC-3: Obsolete rule recorded**
Given the conflict between 08 §4 and IT-08-12 on one side and FT-N-49, data model §9.1, F10 and 09's tests on the other
When the spike ends
Then 08 §4, FT-N-49 and IT-08-12 agree on whether a superseded or deleted revision that is still retained is served to the worker or answered `revision_obsolete`; if the answer is "obsolete", the tree predicate 08 uses is named with its owner (E-13)

**AC-4: Hash encoding recorded**
Given media-shared skeleton open point 1
When the spike ends
Then one encoding of `content_hash` in `ClaimedJob` is written in the worker job protocol contract and the skeleton contract's open point is closed

## Non-Functional Requirements

**Compatibility**
- The hash encoding is the one 09 compares against its BLAKE3 roll-up (AC-72, worker side).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: document outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | ST-08-4, AT-08-1 | Review | One answer per case | AC-59 |
| AC-2 | IT-08-12 | Review | One expected result | AC-72 |
| AC-3 | 08 §4, FT-N-49, IT-08-12 | Review | Documents agree | FT-N-49, AC-71 |
| AC-4 | Worker protocol contract | Review | One encoding | |

## Constraints

- Decisions go into the existing documents; tests.md files are updated by their owners.

## Risks & Mitigation

**Risk 1: "Obsolete" changes the tree predicate**
- *Risk*: if superseded revisions must be obsolete, `is_current_or_retained` is the wrong check for 08.
- *Mitigation*: AC-3 names the predicate and its owner before E-18/c07 starts.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The obsolete question (AC-3) and the hash encoding (AC-4) are not in the epic row's title; they are added because they are 08 decompose-time items that block E-18/c02 and E-18/c07 | E-18 owner | open | Low |
