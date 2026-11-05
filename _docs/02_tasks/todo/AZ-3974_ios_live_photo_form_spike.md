# iOS spike: Live Photo stored form and checkpoint columns

**Task**: AZ-3974_ios_live_photo_form_spike
**Name**: Live Photo stored form spike
**Description**: Decide how a Live Photo and an edited photo are stored on the server and tracked in the checkpoint (`content_identifier`, `group_id`), so PhotoKit backup, reinstall rebuild and FT-P-33 have one written rule
**Complexity**: 2 points
**Dependencies**: AZ-3989_mobile_backup_checkpoint_db
**Component**: 18 ios_app [v1.1]
**Tracker**: AZ-3974
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/components/18_ios_app/description.md` §2 (`group_id`), §4 (`content_identifier`, `group_id` columns).
- `_docs/02_document/components/17_android_app/description.md` §4 — checkpoint schema (`asset`, `linked_from`) of the shared `mobile_backup` crate.
- `_docs/02_document/tests/spec-gaps.md` row IT-18-10; `_docs/02_document/tests/blackbox-tests.md` FT-P-33 (deferred).

## Problem

On iOS one photo can have several resources: the original, an edited render, and for a Live Photo a still plus a short video. 18 says the original and edited renders are separate resources linked by a `group_id`, and adds `content_identifier` and `group_id` columns to the checkpoint. It does not say how a Live Photo is stored (one asset with a linked still and video, two files, or something else), which column holds what, or how the link is represented on the server. IT-18-10 is BLOCKED and FT-P-33 is deferred until this is written.

## Outcome

- A written decision on the stored form of a Live Photo and of an edited photo on iOS, including the server names in `Camera Uploads/<device label>/` and the per-revision `client_meta` that lets a reinstall relink them.
- The meaning and filling rule of the `content_identifier` and `group_id` checkpoint columns.
- The decision recorded in 18 §4, the spec-gaps row IT-18-10, and handed to the E-31 owner for FT-P-33.
- Unblocks E-28/c04 (`group_id` part), E-28/c10 (relinking after reinstall) and the Live Photo leg of IT-18-10.

## Scope

### Included
- Options for the Live Photo stored form with their effect on uploads (bytes, number of server files), on the reinstall rebuild (AC-46: 0 media bytes) and on the asset-id name suffix rule.
- The rule for edited renders: original and render as separate resources linked by `group_id`, and how this matches AC-46's "linked, not duplicate" entries.
- The checkpoint columns: which PhotoKit identifier fills `content_identifier`, how `group_id` is formed, and which existing columns (`linked_from`) they relate to.
- Recording the decision and naming the E-27 change it needs (the checkpoint DB belongs to 17).

### Excluded
- Changing the checkpoint schema or `AssetSource` (E-27 owns `mobile_backup`; see Flagged concerns).
- Writing FT-P-33 (E-31) and implementing the PhotoKit source (E-28/c04).

## Acceptance Criteria

**AC-1: Options written with their costs**
Given an edited photo (original plus render) and a Live Photo (still plus video)
When the options are written up
Then each option states the number of server files, the bytes uploaded on first backup and after a reinstall, and how the resources are linked

**AC-2: Decision recorded**
Given the chosen option
When it is recorded
Then 18 §4 states the stored form and the meaning of `content_identifier` and `group_id`, the spec-gaps row IT-18-10 states the decision and date, and IT-18-10's Live Photo leg has a stated expected result

**AC-3: Downstream named**
Given the decision
When the spike closes
Then the E-27 change needed in the shared checkpoint schema and `AssetSource` is described for the E-27 owner, and the E-31 owner is told what FT-P-33 must assert

## Non-Functional Requirements

**Compatibility**
- The chosen form keeps AC-46: a reinstall uploads 0 media bytes and linked entries stay linked, not duplicated.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Edited photo, Live Photo | Review of the options table | Files, bytes and links per option | AC-46 |
| AC-2 | Chosen option | 18 §4, spec-gaps IT-18-10 | Form and columns stated; IT-18-10 unblocked | IT-18-10, FT-P-33 |
| AC-3 | Decision | Note to E-27 and E-31 owners | Schema change and FT-P-33 assertions named | NFT-RES-28 |

## Constraints

- Server layout and name-collision suffix stay as in 17 (`Camera Uploads/<device label>/`, suffix from the asset id).
- No cross-device dedupe: linking only within the device's own folder (`canonical-names.md`).

## Risks & Mitigation

**Risk 1: Decision forces a schema change late**
- *Risk*: the shared checkpoint schema is built by E-27 for v1.0 without the iOS columns.
- *Mitigation*: AC-3 hands the change to the E-27 owner before E-28/c04 starts.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `content_identifier` and `group_id` columns live in 17's `mobile_backup` checkpoint DB, which 18 does not own; an E-27 change task is needed (none exists in E-27's child list) | E-27 owner | open | Medium |
| FT-P-33 (Live Photo pairing) is deferred and has no fixture yet | E-31 owner | open | Low |
