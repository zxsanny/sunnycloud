# Sharing AccountLifecycleHook: erase an account's links in batches

**Task**: AZ-3702_share_account_erase_hook
**Name**: Share links account erase hook
**Description**: Delete a deleted account's share links in bounded batches when 07's purger asks, and do nothing at account creation
**Complexity**: 1 point
**Dependencies**: AZ-3697_share_create_list_revoke, AZ-3698_share_lookup_open_uniform, AZ-3768_contracts_part1
**Component**: 06 sharing
**Tracker**: AZ-3702
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/account_creation_hooks.md` — how identity calls `initialize` inside account creation (produced by E-15/c05).

## Problem

Deleting an account must remove its shares (AC-27). 07's `DeletionPurger` calls every hook's `erase_batch` until all report nothing left; sharing must answer with its remaining count. Links must stop resolving once their rows are gone, and while the target is a tombstone they already answer as missing.

## Outcome

- Sharing implements `AccountLifecycleHook`: `initialize` writes nothing; `erase_batch(account, limit)` deletes up to `limit` of the account's links and returns `EraseProgress{remaining}`.

## Scope

### Included
- `initialize(tx, account)`: no rows, no I/O (per the account-creation hooks contract).
- `erase_batch(account, limit)`: delete up to `limit` rows of the account, report how many remain.

### Excluded
- Driving the hooks and their order (07, E-17/c06; 07 runs this hook first).
- Registering the hook (E-22/c03).

## Acceptance Criteria

**AC-1: Links are erased in batches (AC-27)**
Given an account with 120 links and another account with 3
When `erase_batch(limit = 50)` is called until it reports 0, and once more
Then the reports are 70, 20, 0 and then 0 again; the other account's 3 links are untouched

**AC-2: Erased links answer as missing**
Given links of the account, some on files already tombstoned
When a visitor opens them before the erase, and opens every link after its row is erased
Then links on tombstoned files answer `NotFound` before the erase, and every erased link answers `NotFound`

**AC-3: Initialize writes nothing**
Given a transaction for a new account id
When the hook's `initialize` runs in it and the transaction commits
Then `share_links` holds no row for the account and the hook did no other work

## Non-Functional Requirements

**Reliability**
- Each batch runs in one short transaction.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | Integration-only | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 120 links; other account 3 | IT-06-10 batches | 70, 20, 0, 0; others untouched | FT-P-19, NFT-SEC-27 |
| AC-2 | Links, some on tombstones | IT-06-10 resolve | Tombstoned `NotFound` before; all erased `NotFound` after | |
| AC-3 | Transaction for a new account id | `initialize` | No rows | |

## Constraints

- The deletion flow through the real server is the pipeline task AZ-3665 (E-22/p12).

## Risks & Mitigation

**Risk 1: Links outlive the account briefly**
- *Risk*: links resolve between the deletion request and the first batch.
- *Mitigation*: identity's `begin_deletion` makes the owner inactive and 07 runs this hook first; a tombstoned target answers as missing meanwhile.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | resolved | Low |
