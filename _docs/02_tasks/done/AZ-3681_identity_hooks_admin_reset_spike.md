# Identity spike: hook registration entry point and sole-admin reset

**Task**: AZ-3681_identity_hooks_admin_reset_spike
**Name**: Identity hook registration and sole-admin reset decisions
**Description**: Name how the composition root hands identity its account-creation hook list, and get the owner's answer on resetting the only admin, so IT-01-17, IT-12-2 and AT-12-2 lose their BLOCKED or open markers
**Complexity**: 2 points
**Dependencies**: None
**Component**: 01 identity
**Tracker**: AZ-3681
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c01

## Problem

Account creation runs every registered `AccountLifecycleHook::initialize` inside one transaction. The hook list is registered late by the composition root (12) to break the wiring cycle, but 01's interface does not say how the list arrives: `register_hooks` is named only in 12. IT-01-17 is therefore partly BLOCKED (`tests/spec-gaps.md`, "IT-01-17 note").

Gap G-12-3 is also open: when the operator resets the only admin, nobody has said whether a last-admin rule applies. The last-admin rule (`LastAdmin`) belongs to 07, and identity has no such error.

## Outcome

- A written decision in `components/01_identity/description.md` (sections AccountLifecycleHook and Ports) that names the entry point through which 12 supplies the hook list, says that 12 is its only caller, says when it is set relative to serving sign-in requests, and says what a sign-in sees if the list was never set.
- The same name in `components/12_operator_cli/description.md` and in IT-01-17 of `components/01_identity/tests.md`, with the BLOCKED marker removed.
- The owner's answer to G-12-3 recorded in 01's description (InviteService / Operator operations) and in 12's description.
- The two `tests/spec-gaps.md` rows (IT-01-17 note, G-12-3) marked resolved with the decision date.
- Unblocks E-15/c05 (IT-01-17), E-15/c08 (`reset_admin`), and on E-22 the hook-list wiring (E-22/c03) and `admin reset` (E-22/c08).

## Scope

### Included
- Compare the two options already present in the docs: late registration on the identity service, or a constructor parameter set by the composition root (the pattern gap G-07-1 chose for 07's `DeletionPurger`). Judge them against the wiring-cycle reason and the one-transaction rule of AC-75. Pick one and write it down.
- Put G-12-3 to the owner with the facts: `reset_admin` revokes that admin's sessions and issues a re-bind invite; the account keeps role admin and state active; until the invite is redeemed nobody can sign in as that admin.
- Update the documents named in Outcome.

### Excluded
- Any code.
- The promote/demote last-admin rule (07, decision 15).
- The hook implementations (02, 05, 07a).

## Acceptance Criteria

**AC-1: Hook entry point named once**
Given 01's and 12's component descriptions
When the decision is written
Then both name the same entry point, name 12 as its only caller, state that it is set before the API accepts sign-in requests, and state the sign-in outcome when the list was never set

**AC-2: IT-01-17 unblocked**
Given the decision of AC-1
When 01's `tests.md` is updated
Then IT-01-17 uses the named entry point and carries no BLOCKED marker, and the spec-gaps row "IT-01-17 note" is marked resolved

**AC-3: Sole-admin reset decided by the owner**
Given the facts of G-12-3 put to the owner
When the owner answers
Then 01's and 12's descriptions state whether `reset_admin` of the only active admin is allowed or refused, and the spec-gaps row G-12-3 is marked resolved with the date

**AC-4: A refusal error is agreed, not invented**
Given the owner's answer is "refuse"
When the refusal is written
Then its error name, HTTP mapping and CLI exit code are agreed with the E-17 owner (who owns `LastAdmin`) and the E-22 owner (exit-code table), and the same text is in 01's and 12's descriptions

## Non-Functional Requirements

**Reliability**
- The chosen entry point must keep account creation one short transaction (AC-75, risk 3 of E-15).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | Spike: no code | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 01 and 12 descriptions | Review of the entry-point text | One name, one caller, timing and unset behaviour stated in both | |
| AC-2 | 01 `tests.md`, `tests/spec-gaps.md` | Review of IT-01-17 and the gap row | No BLOCKED marker; row resolved | |
| AC-3 | Owner answer | Review of both descriptions and the gap row | Same answer in both; row resolved with date | |
| AC-4 | Answer "refuse" only | Review of error name and mappings | Agreed with E-17 and E-22 owners, same text in both | |

## Constraints

- Documents only. No new port, error or table unless the owner chooses "refuse" (AC-4).
- The decision must not make identity import any hook implementer (ADR-015).

## Risks & Mitigation

**Risk 1: Two spikes answer G-12-3 differently**
- *Risk*: E-22/c01 also lists G-12-3.
- *Mitigation*: this task records the single answer; E-22/c01 consumes it (see Flagged concerns).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-12-3 appears in both E-15/c01 and E-22/c01; one answer must serve both tasks | Parent (dedupe at Step 4) / E-22 owner | open | Low |
| G-12-3 needs an owner answer (marked "owner decision: Yes" in `tests/spec-gaps.md`) | Product owner | open | Low |
