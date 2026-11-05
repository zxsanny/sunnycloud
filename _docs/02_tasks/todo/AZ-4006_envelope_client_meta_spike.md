# Spike: file-envelope size and JSON form vs the `revisions.client_meta` CHECK bound [v1.2]

**Task**: AZ-4006_envelope_client_meta_spike
**Name**: Envelope vs client_meta bound
**Description**: Compute the serialised length of a vault file envelope in its JSON form, agree with the tree (data-model) owner the `revisions.client_meta` CHECK bound and the JSON field names, and record them so IT-19-7 is no longer BLOCKED
**Complexity**: 2 points
**Dependencies**: AZ-4004_vault_review_plan_spike
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4006
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_format.md` — file envelope layout (open point 4 is closed here).
- `data_model.md` (`revisions.client_meta`: "size bounded by a `CHECK`"); `tests/spec-gaps.md` row IT-19-7.

## Problem

The wrapped file key of each vault revision travels in `revisions.client_meta`, committed atomically with the revision (19 §11 point 8). `data_model.md` bounds `client_meta` with a CHECK but states no number, and camera backup also writes `client_meta` (device, asset id, link). If the vault envelope does not fit, vault uploads fail at commit; if the field names collide with camera-backup keys, a reinstall rebuild could misread a vault revision.

## Outcome

- The envelope's JSON length computed from `vault_format.md` (constant length) with the encoding chosen.
- The JSON field names and encoding recorded in `vault_format.md`, distinct from the camera-backup keys.
- The CHECK bound value agreed with the E-13 owner and recorded in `data_model.md` by that owner (the change is an E-13 task named here if the bound must change).
- The IT-19-7 row of `tests/spec-gaps.md` resolved with the date.

## Scope

### Included
- Length computation, JSON form proposal, the meeting with the E-13 owner.

### Excluded
- The envelope code (E-29/c08) and any migration change (E-13).

## Acceptance Criteria

**AC-1: Length and form recorded**
Given the frozen (or owner-approved) file envelope layout
When the spike ends
Then `vault_format.md` states the JSON field names, the encoding of binary parts and the constant serialised length in bytes

**AC-2: Bound agreed**
Given the length of AC-1 and the largest camera-backup `client_meta`
When the E-13 owner reviews them
Then the CHECK bound is recorded as a number at least as large as both plus a stated margin, and if the existing bound is smaller, an E-13 task for the change is named with its ref

**AC-3: Keys do not collide**
Given the camera-backup keys of `client_meta` (device, asset id, link to original)
When the vault field names are chosen
Then no vault field name equals a camera-backup key, and the rule is written next to the bound

## Non-Functional Requirements

**Compatibility**
- The bound applies to every client that writes `client_meta` (17, 18, 16).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision record) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Record and data model | Review | Bound recorded; follow-up named | AC-60, IT-19-7 |

## Constraints

- No work moves to E-13 inside this task; only the follow-up is named.

## Risks & Mitigation

**Risk 1: The bound is already shipped in v1.0**
- *Risk*: raising a CHECK in v1.2 needs a migration.
- *Mitigation*: AC-2 names the E-13 change; the migration is additive.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `data_model.md` states no number for the `client_meta` CHECK bound | E-13 owner | open | Medium |
