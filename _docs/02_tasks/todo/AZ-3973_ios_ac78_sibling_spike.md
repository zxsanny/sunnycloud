# iOS spike: AC-78 sibling criterion and shared run-once rules

**Task**: AZ-3973_ios_ac78_sibling_spike
**Name**: AC-78 iOS sibling spike
**Description**: Get the owner's iOS sibling of AC-78 (foreground and background upload one asset once), confirm that the shared mutex and key rules of `mobile_backup` hold on iOS, and attach the criterion to the iOS pipeline
**Complexity**: 1 point
**Dependencies**: None
**Component**: 18 ios_app [v1.1]
**Tracker**: AZ-3973
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c01

## Dependencies

### Document Dependencies
- `_docs/02_document/components/17_android_app/description.md` §5 "One run at a time" — the three-layer design (unique work, in-process mutex, idempotent `client_key`).
- `_docs/02_document/components/18_ios_app/description.md` §11 — "a v1.1 sibling criterion is proposed".
- `_docs/02_document/tests/spec-gaps.md` row IT-18-9; `_docs/02_document/interaction-risks.md` row R11.

## Problem

AC-78 says that when the foreground app and the background worker upload the same asset at once, exactly one server file is created. It is written for Android only. On iOS the same race exists (app open or foreground observer versus `BGProcessingTask` or `BGAppRefreshTask`), and 18 reuses the same `mobile_backup` design, but no iOS criterion exists. IT-18-9 is therefore BLOCKED, and the iOS pipeline (E-28/p01) carries no interaction AC for R11.

## Outcome

- An owner-approved iOS sibling of AC-78 in `acceptance_criteria.md`, with its own id and the [v1.1] tag.
- A written confirmation that the shared design applies on iOS unchanged, or a list of the iOS differences the owner accepts.
- IT-18-9 traced to the new id; the spec-gaps row closed; the R11 row of `interaction-risks.md` names the iOS pipeline as owner of the iOS leg.
- Unblocks E-28/c07 (IT-18-9) and the iOS pipeline E-28/p01.

## Scope

### Included
- A proposed criterion text for the owner, worded like AC-78: foreground and background runs on iOS that upload the same asset at once create exactly one server file.
- Checking the three layers of 17 §5 against iOS: (1) the trigger layer (iOS has no unique work; triggers are app open, foreground observer and two background task kinds), (2) the in-process mutex (background tasks run inside the app process, so a second `run_backup` returns `Skipped(AlreadyRunning)`), (3) idempotent uploads (`client_key` = `derive_client_key(device_id, asset_id, file_hash)`, random batch `commit_id` stored before the first `stage_small`).
- Recording the result in the acceptance criteria, the 18 tests traceability (IT-18-9), the spec-gaps row IT-18-9 and the R11 row of `interaction-risks.md`.

### Excluded
- Writing or running IT-18-9 (E-28/c07) and the pipeline test (E-28/p01).
- Any change to the shared crate's mutex or key rules (E-27/c06, owned by 17).

## Acceptance Criteria

**AC-1: Criterion proposed with the design check**
Given 17 §5 and 18 §5
When the spike writes its note
Then the note states the proposed criterion text and, for each of the three layers, whether it applies on iOS as is or what differs (trigger layer at least)

**AC-2: Owner decision recorded**
Given the owner's answer
When it is recorded
Then `acceptance_criteria.md` holds the iOS sibling with its id and [v1.1] tag, IT-18-9's traceability row names that id instead of "BLOCKED", and the spec-gaps row IT-18-9 states the decision and date

**AC-3: Attached to the iOS pipeline**
Given the new criterion
When the interaction owners are updated
Then the R11 row of `interaction-risks.md` names the iOS pipeline E-28/p01 as owner of the iOS leg and E-28/c07 as keeper of the component part, and the flagged concern on E-28/p01 is marked for its owner to turn into an interaction AC

**AC-4: No answer keeps the gap visible**
Given no owner answer when the spike closes
When the spike is closed
Then the spec-gaps row stays `open`, IT-18-9 stays BLOCKED, and the note says which tasks wait on it

## Non-Functional Requirements

**Reliability**
- The proposed criterion must be testable without wall-clock waiting for a background window (debug trigger command, as in IT-18-4).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 17 §5, 18 §5 | Review of the design note | Criterion text plus a verdict per layer | IT-18-9 |
| AC-2 | Owner answer | Acceptance criteria, 18 traceability, spec-gaps row | New id present; IT-18-9 no longer BLOCKED | IT-18-9 |
| AC-3 | New criterion | `interaction-risks.md` R11 row | iOS leg owned by E-28/p01, component part by E-28/c07 | |
| AC-4 | No answer | Spec-gaps row | `open`; waiting tasks listed | |

## Constraints

- The criterion is the owner's; this task proposes and records.
- iOS reuses the one shared uploader; the sibling must not require a second protocol path (AC-52).

## Risks & Mitigation

**Risk 1: iOS has no unique-work layer**
- *Risk*: layer 1 of 17's design has no direct iOS equivalent, so only the mutex and idempotent keys protect against duplicates.
- *Mitigation*: AC-1 states this explicitly; IT-18-9 asserts the server file count, which holds even if layers 1 and 2 are bypassed.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AC-78 has no iOS sibling; IT-18-9 and the iOS pipeline's R11 leg wait on the owner's text | Owner | open | Low |
