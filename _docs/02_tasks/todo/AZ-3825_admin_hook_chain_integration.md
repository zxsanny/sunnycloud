# Integration pass of the full v1.0 erase-hook chain

**Task**: AZ-3825_admin_hook_chain_integration
**Name**: Full hook chain integration
**Description**: Re-run the purger's drain and resume cases with the real media-jobs erase hook in the chain, so an account with jobs and derived files is erased completely in the documented order
**Complexity**: 2 points
**Dependencies**: AZ-3820_deletion_purger, AZ-3845_media_janitors_erase_hook
**Component**: 07 admin
**Tracker**: AZ-3825
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/07_admin/deletion_purger.md` — driving rules (E-17/c06).
- `_docs/02_document/contracts/08_media_jobs/maintenance_ports.md` — the 08 erase hook (E-18/c10).

## Problem

E-17/c06 proves the purger with the hooks of 06, 02, 03, 05 and 07a. Media jobs and derived files (08) join the chain later; until IT-07-9 runs with 08's real hook, thumbnails and transcodes of a deleted account (AC-27) are unproven at the component boundary.

## Outcome

- IT-07-9 and IT-07-10 pass with the v1.0 hook list 06, 02, 03, 08, 05, 07a and the media part of the data set.
- Jobs and derived rows of the account are gone; derived blobs were queued `Due::Now` and their files removed by 04's GC.

## Scope

### Included
- The media part of IT-07-9's data set (30 photo and video jobs, 60 derived rows) and the 08 position in IT-07-10.

### Excluded
- The vault hook (20): it joins the chain when E-30/c05 lands for v1.2; no stand-in is built.

## Acceptance Criteria

**AC-1: Full v1.0 chain drains**
Given the IT-07-9 account including 30 jobs and 60 derived rows, and recording wrappers around the real hooks in the order 06, 02, 03, 08, 05, 07a
When the purger runs until the account is gone
Then 08's hook is first called after 03 reported 0, 05 and 07a after 08 reported 0, the table sweep (including jobs and derived) counts 0 rows for the account, derived blobs were queued with `Due::Now` and 0 derived blob files remain after GC, and `account_deleted` is audited once

**AC-2: Resume at the media position**
Given a decorator that fails the first call of 08's hook
When cycles continue
Then the failure is logged with the account id and position 4, the next cycle continues, and the final sweep counts 0 rows

## Non-Functional Requirements

**Reliability**
- Same bounds as E-17/c06: batch limit respected; no purger transaction over 1 s.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: integration pass | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | tree-small plus media-subset (DS-03) | IT-07-9 with 08 | Order kept; 0 rows incl. jobs and derived; 0 files | FT-P-19, NFT-SEC-27 |
| AC-2 | Failing decorator at 08 | IT-07-10 (position 08) | Resumes; 0 rows | NFT-RES-30 |

## Constraints

- Real hooks only; the decorators delegate to them.

## Risks & Mitigation

**Risk 1: 08 erases derived rows before their jobs**
- *Risk*: a job finishing during erase re-creates a derived row.
- *Mitigation*: 08's hook deletes jobs first; AC-1 sweeps after the last cycle.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The epic row says "and E-30 stub"; no stub is built. How the vault hook is left out of v1.0 builds is open (module-layout exception 6) | E-17 / E-30 / E-22 owners | open | Low |
