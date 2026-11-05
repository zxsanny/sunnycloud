# Transfer spike: NothingToSend after a lost commit response

**Task**: AZ-3732_transfer_spike_nothing_to_send
**Name**: Transfer NothingToSend spike
**Description**: Confirm with the E-13 owner the exact rule by which `start_upload` answers `NothingToSend` for a parent, name and content hash after a lost commit response, and record it
**Complexity**: 1 point
**Dependencies**: None
**Component**: 03_transfer
**Tracker**: AZ-3732
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md` — `start_upload` outcomes.
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `find_live_node`.
- `_docs/02_document/tests/spec-gaps.md` — row IT-13-16 (B).

## Problem

A large upload has no receipt: if its commit response is lost, the client heals by calling `start_upload` again with the same `client_key` and the file hash, and expects `NothingToSend{revision}`. For a new file this works only if 03 matches the live node at (parent, name) and compares its current revision's hash. IT-13-16 (B) in the sync core is blocked on that rule (13 risk 2); otherwise the retry creates a conflict copy of the file the user already has.

## Outcome

- A written rule, agreed with E-13, for each case of `start_upload` with a `content_hash`.
- IT-13-16 (B) unblocked or re-scoped, and the test lines added to E-14/c04.

## Scope

### Included
- The outcome for: live file at the name with the same current hash; live file with another hash; tombstoned file at the name; folder at the name; no node; a `base_revision` given.
- Recording in the upload contract and in the IT-13-16 (B) row of `tests/spec-gaps.md`.

### Excluded
- Implementing the check (E-14/c04); the staged path, where receipts heal lost responses (E-14/c09).

## Acceptance Criteria

**AC-1: Rule covers every case**
Given the six cases in Scope
When the rule is written
Then each case has one stated outcome (`NothingToSend{revision}`, `Started`, or a named error), with the node matched by `name_key` through `TreeReader::find_live_node`

**AC-2: Recorded and unblocked**
Given the agreed rule
When it is recorded
Then the upload contract lists the rule, the IT-13-16 (B) row names the outcome for a new file after a lost response, and E-23's owner of IT-13-16 is named in that row

**AC-3: Test lines assigned**
Given the rule
When the spike closes
Then IT-03-1's `NothingToSend` line has one case per outcome in AC-1, assigned to E-14/c04

## Non-Functional Requirements

**Performance**
- The rule needs at most one tree lookup and one revision read before the session transaction.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Six cases | Review of the rule | One outcome each | IT-03-1 |
| AC-2 | Agreed rule | Contract and IT-13-16 (B) row | Rule recorded; owner named | IT-13-16, AC-46 |
| AC-3 | Agreed rule | IT-03-1 case list in E-14/c04 | One case per outcome | |

## Constraints

- No commit-by-hash across nodes exists (`canonical-names.md`); the rule applies only to the node at the given parent and name.

## Risks & Mitigation

**Risk 1: Name matched case-sensitively**
- *Risk*: a client retry with different case misses the node.
- *Mitigation*: AC-1 requires matching by `name_key`.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-13-16 (B) is BLOCKED until this rule is recorded | E-14 / E-23 | open | Low |
