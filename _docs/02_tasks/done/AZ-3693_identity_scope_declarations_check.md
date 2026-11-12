# Scope declarations for identity-owned operations and the CI scope check

**Task**: AZ-3693_identity_scope_declarations_check
**Name**: Identity scope declarations check
**Description**: Make CI fail when an identity-owned operation, or one of the four AC-57 session-only operations, is declared with a different scope than 01's scope model assigns
**Complexity**: 2 points
**Dependencies**: AZ-3683_authenticator_scopes_cache, AZ-3827_route_registry_openapi, AZ-3828_identity_admin_handlers, AZ-3830_share_media_handlers
**Component**: 01 identity
**Tracker**: AZ-3693
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/authenticator.md` — the scope table this check enforces (produced by E-15/c03).

## Problem

01 owns the scope model, and its description requires that every OpenAPI operation of the public listener declares one scope with CI failing otherwise. E-21/c11 already checks that every operation declares some auth class. Nothing checks that the declared class is the right one: a device token reaching approve-device, create-share, delete-account or link-provider would break AC-57 silently.

## Outcome

- A scope table for every identity-owned operation (01 §3) kept beside the authenticator contract.
- A CI check that reads the generated OpenAPI document from E-21's route registry and fails, naming the operation, when a declaration differs from that table or when one of the four AC-57 operations is not session-only.

## Scope

### Included
- Expected class per identity operation: `Public` for providers, login attempts, provider callback, mobile code exchange, refresh, device-grant start and device-token poll; session-only for sessions, identity-link attempts, device-grant lookup and decision, devices list and revoke by id; the device's own revoke (`DELETE /api/v1/devices/me`) as recorded in the authenticator contract; `Admin` for admin invites.
- The four AC-57 operations owned elsewhere (create-share from 06, delete-account from 07) checked as session-only as well.
- The check runs in the same CI job as E-21's registry and OpenAPI parity test.

### Excluded
- The presence check "every operation declares a class" and the route/OpenAPI parity test (E-21/c11).
- The CSRF and two-account matrices (E-21/c18).
- Worker routes on the internal listener (08, E-21/c15).

## Acceptance Criteria

**AC-1: Identity operations carry their assigned scope**
Given the generated OpenAPI document of the release build
When the check runs
Then every identity-owned operation listed in Scope declares exactly the expected class, and the check passes

**AC-2: The AC-57 operations stay session-only (AC-57)**
Given the same document
When the check reads approve-device, create-share, delete-account and link-provider
Then each declares the session-only class, so a device token cannot pass it

**AC-3: Drift fails the build with a clear message**
Given a test copy of the document in which one identity operation is changed to `SessionOrDevice` and another loses its class
When the check runs on it
Then it fails and names both operations and their expected classes

## Non-Functional Requirements

**Reliability**
- The check reads the same generated document E-21 publishes; it keeps no second route list of its own beyond the expected-class table.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Check over a mutated document | Fails naming each drifted operation |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Release OpenAPI document | CI scope check | Pass; every identity operation as expected | |
| AC-2 | Release OpenAPI document | AC-57 operations | Session-only on all 4 | FT-N-24, NFT-SEC-07 |
| AC-3 | Mutated copy | CI scope check | Fails naming both operations | |

## Constraints

- Paths in 01 §3 are indicative for components other than 03, 07 and 08 (11 §8); the check matches operations by the owner port method the registry records, not by path text.

## Risks & Mitigation

**Risk 1: Two checks drift apart**
- *Risk*: E-21's presence check and this value check disagree on class names.
- *Mitigation*: both read the class names from the registry; the mapping from 01 scopes to 11 classes is in the authenticator contract.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Overlap with E-21/c11 ("every operation declares AuthClass"): resolved by splitting presence (E-21) from expected values (this task) | E-21 owner | resolved | Low |
| 11 has a `DeviceOnly` class that 01's scope model lacks (see authenticator contract); the expected class of `DELETE /api/v1/devices/me` follows that decision | E-15 and E-21 owners | open | Medium |
