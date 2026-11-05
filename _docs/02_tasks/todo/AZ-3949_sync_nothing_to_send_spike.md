# Sync core spike: lost-commit recovery for a new file (IT-13-16 B)

**Task**: AZ-3949_sync_nothing_to_send_spike
**Name**: Lost-commit recovery for a new file
**Description**: Take 03's recorded `NothingToSend` rule for a new file at `(parent, name, hash)` and write down what the client does in each case after a lost commit response, so IT-13-16 (B) gets a stated expected result
**Complexity**: 2 points
**Dependencies**: AZ-3732_transfer_spike_nothing_to_send
**Component**: 13_sync_core
**Tracker**: AZ-3949
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md` — `start_upload` outcomes and the `NothingToSend` rule recorded by E-14/c02.
- `_docs/02_document/tests/spec-gaps.md` — row IT-13-16 (B).
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `upload` outcomes and lost-commit recovery.

## Problem

A large upload has no receipt. When a commit response is lost, the client calls `start_upload` again with the same `client_key` and hash and expects `NothingToSend{revision}`. For a new revision of an existing node this works. For a brand-new file it works only if 03 matches the live node at `(parent, name)` and compares its current hash (13 risk 2). The 03 side is decided by E-14/c02; the client side is not: IT-13-16 (B) is BLOCKED and the large-upload executor (E-23/c09b) cannot state its recovery result. AC-78 and AC-46 depend on it (one server file after a retry).

## Outcome

- A written client rule for every case of E-14/c02's table: what `upload` returns after a lost commit response (`NothingToSend{revision}`, a new session, or a named error), recorded in the IT-13-16 (B) row of `tests/spec-gaps.md` and in the open point 7 of the `sync_api` contract.
- IT-13-16 (B) unblocked with its expected result, or re-scoped with the owner if 03 cannot return `NothingToSend` for a new file.
- E-23/c09b and E-23/c18a can assert the result.

## Scope

### Included
- Reading E-14/c02's rule and mapping each of its six cases (live file same hash, live file other hash, tombstoned file, folder at the name, no node, base revision given) to the client outcome after a lost response.
- The case where the retry finds another device's file at the same name (the client must not create a conflict copy of its own committed file).
- Recording the result and the IT-13-16 (B) expected outcome.

### Excluded
- Implementing recovery (E-23/c09b) and the server check (E-14/c04).
- The staged path, where receipts already heal lost responses (E-23/c10, E-14/c09).

## Acceptance Criteria

**AC-1: Every server case has one client outcome**
Given E-14/c02's recorded rule with its six cases
When the client rule is written
Then each case names exactly one result of `upload` after a lost commit response (`NothingToSend{revision}`, a new upload, or a typed error from the `sync_api` contract), and the case "brand-new file, response lost" names how many revisions exist afterwards (expected 1)

**AC-2: IT-13-16 (B) unblocked or re-scoped**
Given the client rule
When it is recorded
Then the IT-13-16 (B) row of `tests/spec-gaps.md` states the expected result, the date and E-23/c09b as the implementing task; if 03 cannot answer `NothingToSend` for a new file, the row states the owner's chosen alternative or stays `open` with IT-13-16 (B) still BLOCKED

**AC-3: Downstream informed**
Given the recorded rule
When the spike closes
Then the `sync_api` contract open point 7 and the flagged concern on E-23/c09b point at the recorded rule

## Non-Functional Requirements

**Reliability**
- The rule keeps AC-78: a foreground and a background run of the same asset end with one server file.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | E-14/c02 rule | Review of the case table | One client outcome per case; 1 revision for the new-file case | IT-13-16, AC-46, AC-78 |
| AC-2 | Client rule | IT-13-16 (B) row | Expected result, date, implementing task | FT-P-32 |
| AC-3 | Recorded rule | Contract and concern pointers | Updated | |

## Constraints

- No commit-by-hash across nodes exists (`canonical-names.md`): the rule applies only to the node at the given parent and name.
- The client never retries with a new name; the server assigns conflict-copy names (ADR-005).

## Risks & Mitigation

**Risk 1: 03 cannot match a new file**
- *Risk*: the retry creates a conflict copy of the file the user already has.
- *Mitigation*: AC-2 forces an explicit owner choice instead of a silent behaviour.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-13-16 (B) is BLOCKED until E-14/c02 records the server rule (13 risk 2) | E-14 / E-23 | open | Low |
