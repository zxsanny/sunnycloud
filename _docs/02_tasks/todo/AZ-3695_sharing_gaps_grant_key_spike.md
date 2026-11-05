# Sharing spike: limited-link password rule, expiry boundary and grant-key check entry point

**Task**: AZ-3695_sharing_gaps_grant_key_spike
**Name**: Sharing gap decisions
**Description**: Close G-06-2 and G-06-3 and name where a missing share grant key stops the server, so IT-06-7, IT-06-8 and IT-06-12 have stated expectations
**Complexity**: 2 points
**Dependencies**: None
**Component**: 06 sharing
**Tracker**: AZ-3695
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c01

## Problem

Three points in 06 are open (`tests/spec-gaps.md`):
- G-06-2: what a correct password gets while the per-link wrong-password counter is limited. IT-06-8 expects `RateLimited`; the description does not say.
- G-06-3: the expiry boundary. IT-06-7 fixes `expires_at > now` as live; the description does not say.
- IT-06-12 note: the server must refuse to start without `share_grant_key`, but no constructor or validation entry point is named, so IT-06-12 is BLOCKED.

## Outcome

- Decisions written into `components/06_sharing/description.md` (sections PublicShareService, Implementation Details, Configuration keys) and the matching E-16 assumption.
- IT-06-12 in `components/06_sharing/tests.md` rewritten as a runnable component test against the named entry point, with its expected result; BLOCKED removed.
- The three `tests/spec-gaps.md` rows marked resolved with the date.
- Unblocks E-16/c02 (grant key loading), E-16/c06 (expiry), E-16/c07 (limited-link rule), and the last step of AZ-3662 AC-2.

## Scope

### Included
- G-06-2: choose between "a correct password while the link is limited is `RateLimited`" (what IT-06-8 expects, and what NFT-SEC-05 step 2 does for device approvals) and "a correct password is accepted"; record the reason (hash-cost DoS, NFT-SEC-06).
- G-06-3: record the boundary as `expires_at > now` is live and `expires_at <= now` is `NotFound`, or the alternative with its reason; time from the injectable clock.
- Name the entry point that refuses start when `share_grant_key` is missing or unusable: which constructor or config validation, who calls it at `serve` start (12), and what "unusable" means (for example empty or too short), agreed with the E-01 and E-22 owners.

### Excluded
- Code. The vault-file refusal rule (owner item "Vault membership", E-30, v1.2).

## Acceptance Criteria

**AC-1: G-06-2 decided**
Given the two options and the existing tests
When the decision is written
Then 06's description states the outcome of a correct password while the per-link counter is limited, and IT-06-8's expectation matches it

**AC-2: G-06-3 decided**
Given the expiry tests
When the decision is written
Then 06's description states which side of `expires_at` is live, IT-06-7's expectation matches it, and the E-16 assumption on the boundary is no longer marked as a default

**AC-3: Grant-key check has a named entry point**
Given the requirement that the server refuses to start without `share_grant_key`
When the decision is written
Then 06's description names the entry point, its caller at start, and the refusal cases; IT-06-12 states a runnable component expectation (start refused, message names the key) and carries no BLOCKED marker

**AC-4: Gap index updated**
Given AC-1 to AC-3
When `tests/spec-gaps.md` is updated
Then the rows G-06-2, G-06-3 and the IT-06-12 note are marked resolved with the date

## Non-Functional Requirements

**Reliability**
- Whatever G-06-2 decides must keep the number of Argon2 verifications per link at most 10 per window (ST-06-6).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | Spike: no code | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 06 description, IT-06-8 | Review | Rule stated; test matches | NFT-SEC-06 |
| AC-2 | 06 description, IT-06-7 | Review | Boundary stated; test matches | NFT-RES-25 |
| AC-3 | 06 description, IT-06-12 | Review | Entry point, caller, cases named; no BLOCKED | |
| AC-4 | `tests/spec-gaps.md` | Review | Three rows resolved with date | |

## Constraints

- Documents only; no new error names unless agreed with the E-02 owner.

## Risks & Mitigation

**Risk 1: Grant-key check placed twice**
- *Risk*: E-01 secret validation and 06 both check the key, with different rules.
- *Mitigation*: AC-3 names one entry point and its caller.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The grant-key entry point touches E-01 (secret loading) and E-22 (start sequence); both owners must agree | E-01 / E-22 owners | open | Low |
