# Account view and the account deletion request

**Task**: AZ-3819_account_view_deletion_request
**Name**: Account view and deletion request
**Description**: A user sees their own account and quota, and can request deletion of their account from a session with an explicit confirmation; the request only marks the account `deleting`, revokes its credentials and returns the deadline
**Complexity**: 3 points
**Dependencies**: AZ-3814_admin_gaps_spike, AZ-3816_admin_crate_skeleton, AZ-3683_authenticator_scopes_cache, AZ-3690_account_admin_and_guard, AZ-3751_quota_admin_lifecycle, AZ-3707_tree_reader, AZ-3709_tree_publish_core, AZ-3697_share_create_list_revoke
**Component**: 07 admin
**Tracker**: AZ-3819
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/account_admin_and_guard.md` — `begin_deletion`, `AccountGuard` (E-15/c10).
- `_docs/02_document/contracts/01_identity/authenticator.md` — rejection of revoked credentials (E-15/c03).
- `_docs/02_document/contracts/07a_quota/quota-admin.md` — `view` (E-11/c03).
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `root_node` (E-13/c03).

## Problem

Users need their own account details for the web account page, and a safe way to delete their account (AC-27, AC-30). The request must be refused for a device token (AC-57) and without the right confirmation, must stop all access at once (AC-21), and must not do the erasure itself: that is the purger's job, in short batches.

## Outcome

- `AccountService::view(principal)` returns `AccountView{id, display_name, role, identities, quota, root_node_id}` of the caller only; `quota_view` returns the caller's limit, used and reserved from 07a.
- `AccountDeletionService::request(principal, confirmation)` returns `DeletionAccepted{deadline}` with deadline = now + `account_delete_deadline` (injected clock).
- The request runs one short transaction that calls `begin_deletion` (state `deleting`; 01 revokes sessions, device tokens and grants) and nothing else.
- A device-token principal gets `Forbidden`; a wrong confirmation gets `ConfirmationMismatch`; in both cases nothing changes.

## Scope

### Included
- The view, the quota view and the deletion request with its two refusals.
- An audit entry for the request with ids only.

### Excluded
- The last-admin guard in the request transaction (E-17/c04).
- Erasure and finalization (E-17/c06); HTTP routes (E-21/c12); `admin user delete` (E-22/c09).

## Acceptance Criteria

**AC-1: Own view only**
Given account A with a Google and a GitHub identity, and account B
When A and B call `view` and `quota_view`
Then A's view lists both identities and the root node id equal to the tree's root node of A, A's quota view equals 07a's view of A (limit, used, reserved), and B's calls never return A's values

**AC-2: Device token refused (AC-57)**
Given user U's device-token principal and the right confirmation
When `request` is called
Then it returns `Forbidden`, U stays active and every credential of U still works

**AC-3: Confirmation must match**
Given user U's session principal
When `request` is called with an empty, a whitespace-only, a wrong-case confirmation, and with another account's id
Then each call returns `ConfirmationMismatch` and U's state is unchanged

**AC-4: Request marks deleting and ends access (AC-21, AC-27)**
Given user U with files, a share, 2 sessions and 1 device token, and `account_delete_deadline` of 30 s
When U's session calls `request` with the right confirmation
Then it returns within 1 s with deadline = now + 30 s, U's state is `deleting`, all 3 credentials are rejected, U's nodes, blobs and share still exist right after the call, a commit attempt by U fails through the account guard, and the audit entry carries ids only

## Non-Functional Requirements

**Performance**
- The request returns within 1 s (IT-07-8).

**Reliability**
- Requests by the account fail within 5 s of the request (AC-21).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Confirmation comparison with the rule recorded by E-17/c01 | Only the exact value passes |
| AC-4 | Deadline from the injected clock | now + `account_delete_deadline` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Accounts A (two identities) and B | IT-07-7 | Own data only; root node matches | AC-59 |
| AC-2 | Device token, right confirmation | IT-07-8; ST-07-2 step 1 | `Forbidden`; state active | NFT-SEC-08 |
| AC-3 | Session; four bad confirmations | IT-07-8; ST-07-2 steps 2-3 | `ConfirmationMismatch`; no change | |
| AC-4 | U with files, share, 3 credentials | IT-07-8 | Deleting; credentials rejected; data still present; commit refused | FT-P-19, NFT-SEC-27 |

## Constraints

- "Nothing else happens in the request": no erase hook and no blob work in this transaction (07 §3).
- Owns ST-07-2; the security suite task E-17/c09 does not repeat it.

## Risks & Mitigation

**Risk 1: Data left reachable after the request**
- *Risk*: a cached principal keeps working after `begin_deletion`.
- *Mitigation*: AC-4 checks every credential right after the call; the cache bound is 01's (AC-21).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The exact confirmation value is not specified (ST-07-2 implies an exact, case-sensitive value tied to the account); decided by E-17/c01 | E-17 owner | open | Medium |
| `request` takes a principal, but `admin user delete` (operator) has none (contracts part 1 open point 5); decided by E-17/c01 | E-17 / E-22 owners | open | Medium |
| No port named in 07's Requires gives the caller's display name and linked identities for `AccountView` (`AccountAdmin` lists accounts only) | E-15 / E-17 owners | open | Medium |
| `ConfirmationMismatch` has no code or status in the shared status table | E-02 / E-17 owners | open | Medium |

## Contract

This task produces `_docs/02_document/contracts/07_admin/account_services.md` (view, quota view and the deletion request without the last-admin rule).
Consumers MUST read that file — not this task spec — to discover the interface.
