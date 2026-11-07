# Assign 409 vs 412 for `need_blocks` and `edited_since` (spike, G-11-1)

**Task**: AZ-3800_status_assignment_spike
**Name**: Status assignment spike
**Description**: Get the E-13 and E-14 owners' decision on the HTTP status of `need_blocks` and `edited_since` (409 or 412), and record the owners' answer on the batch-too-large status (413 or 422), so the error mapper and its tests have one table
**Complexity**: 2 points
**Dependencies**: None
**Component**: 11 http_api
**Tracker**: AZ-3800
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c02

## Problem

11 §6 maps `PreconditionFailed` (`need_blocks`, `edited_since`) to "409/412 per code" without saying which code gets which (gap G-11-1); IT-11-1 can only assert "409 or 412". Helper 02's status table and the 02 and 03 specs list 409 for both. A second, related mismatch: F5 (`system-flows.md`), FT-N-48 and pipeline AZ-3656 expect 413 for a batch over `batch_max_entries`, while helper 02, 02 and 03 map `batch_too_large` to 422. Clients (13, 16) key retries on these statuses, so they must be settled before the mapper ships.

## Outcome

- One recorded status for `need_blocks` and one for `edited_since`, agreed with the E-13 and E-14 owners, with the reason.
- The owners' answer on the batch-too-large status (one status for "more entries than `batch_max_entries`", and whether a body over the 4 MiB `Batch` class is a separate 413).
- The decisions written into the G-11-1 row of `tests/spec-gaps.md`, the 11 description §6 table and the open points of `contracts/11_http_api/problem_details.md`; a change request to the E-02 owner when the `sc_error` table must change.

## Scope

### Included
- Reading the 02, 03, 05, 13 and 16 specs for every place that relies on these statuses, and listing them in the record.
- Asking the E-13, E-14 and E-02 owners; recording the answer and its date.
- Naming the tests that change: IT-11-1, FT-N-48 step 4, pipeline AZ-3656, IT-13 retry cases.

### Excluded
- Implementing the mapper (E-21/c09) or changing the `sc_error` table (E-02).
- Any other code's status.

## Acceptance Criteria

**AC-1: G-11-1 decided and recorded**
Given the current sources (11 §6 "409/412 per code"; helper 02, 02 and 03 say 409)
When the owners answer
Then the G-11-1 row of `tests/spec-gaps.md` and the 11 §6 table state one status for `need_blocks` and one for `edited_since`, the date and the owners; if the owners do not answer, the row stays open with the interim rule (409 for both, as helper 02 lists) and the tasks it touches

**AC-2: Batch-too-large answer recorded**
Given F5, FT-N-48 and AZ-3656 (413) versus helper 02 and 03 (422)
When the owners answer
Then the record states the status for too many entries and for a body over the `Batch` class limit, and lists the documents that must change (system-flows F5, FT-N-48, AZ-3656 or helper 02 and 03)

**AC-3: Consumers named**
Given the decisions
When the record is complete
Then it names every test and task that asserts these statuses (IT-11-1, FT-N-48, AZ-3655, AZ-3656, AZ-3659, E-23/c06) and the `problem_details.md` open points are updated

## Non-Functional Requirements

**Compatibility**
- The decision must hold for the whole of `/api/v1` (a later change is breaking for clients).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (decision task) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Owner answer | Document review of spec-gaps G-11-1 and 11 §6 | One status per code, dated | IT-11-1 |
| AC-2 | Owner answer | Document review | One status per case; documents to change listed | FT-N-48 |
| AC-3 | Decisions | Consumer list | All asserting tests and tasks named | |

## Constraints

- The spike records decisions; it does not choose a status itself.

## Risks & Mitigation

**Risk 1: Owners unavailable**
- *Risk*: the mapper (E-21/c09) waits on the answer.
- *Mitigation*: the interim rule (helper 02's 409) is written down so c09 can proceed and change one table row later.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-11-1: 409 vs 412 for `need_blocks` and `edited_since` | E-13, E-14, E-21 owners | resolved 2026-11-07: 409 for both | Medium |
| Batch too large: 413 (F5, FT-N-48, AZ-3656) vs 422 (helper 02, 03) | E-14, E-02 owners | resolved 2026-11-07: 422 (a body over the 4 MiB class is a separate 413) | Medium |

## Resolution (2026-11-07)

Owner-approved decisions:
- G-11-1: `need_blocks` and `edited_since` are both 409, as helper 02 and the 02 and 03 specs list; 412 is reserved for `If-Match` header preconditions.
- Batch too large: `batch_too_large` (more entries than `batch_max_entries`) is 422; a body over the 4 MiB `Batch` class is a separate 413 from the HTTP layer. Documents that say 413 for too many entries are wrong.
- Recorded in: `components/11_http_api/description.md` (section 6 table), `contracts/11_http_api/problem_details.md` (open points 1 and 2 closed), `contracts/13_sync_core/sync_api.md`, `contracts/03_transfer/commit_service.md`, and the flagged concerns of AZ-3829, AZ-3754, AZ-3939, AZ-3940 and AZ-3762.
