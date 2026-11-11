# Contract: account-creation hooks (`AccountLifecycleHook::initialize` as called by identity)

**Component**: 01_identity
**Producer task**: B04-05_invite_redemption_account_creation (E-15/c05)
**Consumer tasks**: E-11/c03 (quota row), E-12/c06 (account feed), E-13/c13 (root node), E-16/c08 (sharing, no-op), E-22/c03 (registers the list), E-15/c08 (re-bind redemption)
**Version**: 1.1.0
**Status**: draft (the registration entry point is named; freezes with the first implementation, E-15/c05)
**Last Updated**: 2026-11-11

## Purpose

Identity is the only caller of `AccountLifecycleHook::initialize`. This file states when and how it calls it, and what a hook implementer must and must not do. The trait signature lives in `contracts` (E-00/c03, "registries and lifecycle"); `erase_batch` is driven by 07 and is not covered here.

## Shape

| Name | Signature (behavioural) | Throws / Errors | Blocking? |
|------|-----------|-----------------|-----------|
| `initialize` | `(tx: &mut DbTx, account_id: AccountId) -> Result<(), error type from contracts>` | any error aborts the account creation | async, inside identity's transaction |
| registration | `register_hooks(&self, hooks: Vec<Arc<dyn AccountLifecycleHook>>)` on the identity service (inherent, not in `contracts`) | `HooksAlreadyRegistered` on a second call | called once by 12 at start, before the listeners accept requests |

Call order inside one account-creation transaction:

1. insert the `accounts` row (identity columns, state `active`) and the `identities` row;
2. call `initialize` of every registered hook, in registration order;
3. redeem the invite with the single conditional update;
4. commit. Any error in 1 to 3 rolls back the whole transaction.

## Invariants

- `initialize` is called exactly once per created account, inside the transaction that creates it, after the `accounts` and `identities` rows exist and before the invite is redeemed.
- A hook writes only its own rows and only through the given `tx`; it does not commit, roll back or open another transaction.
- A hook does no I/O outside that transaction (no network, no file, no blob store) and returns quickly.
- If any hook returns an error, no row of any hook, no account, no identity and no redemption remains; the invite stays redeemable; the sign-in returns a typed failure.
- When two redemptions of one invite race, the loser's transaction rolls back, so the loser's hooks leave no row (AC-75).
- `initialize` is not called when no account is created: a re-bind invite (`target_account_id` set, `admin reset`) attaches an identity to an existing account.
- Identity never imports a hook implementer; the list comes from the composition root (12).
- The list is set once. If account creation runs while it was never set, it fails closed: typed failure, nothing written, invite redeemable; an unset list is never treated as empty.

## Non-Goals

- Not covered: `erase_batch`, its order and the `DeletionPurger` (07).
- Not covered: what each hook creates (quota row, account feed with its epoch, root node); each implementer's spec owns that.

## Versioning Rules

- **Breaking changes** (moving the call outside the transaction, changing the order of steps 1 to 3, calling `initialize` for re-bind) require a new major version.
- **Non-breaking additions** (a new hook in the list, a new optional error variant hooks may return) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| valid-creation | unknown identity with a valid invite, hooks 02/05/07a | account, identity, root node, feed, quota in one commit | AC-1 of the producer task |
| failing-hook | one hook errors on first call | no rows at all; invite still redeemable; retry succeeds | IT-01-17 |
| concurrent-loser | 2 and 16 racers on one invite | one account; losers' hooks leave no rows | IT-01-8, NFT-SEC-48 |
| rebind | invite with `target_account_id` | no `initialize` call; no new account | IT-01-9 |
| unset-list | `register_hooks` never called, valid invite | typed failure; no rows; invite redeemable | IT-01-17 |
| second-registration | `register_hooks` twice | second call `HooksAlreadyRegistered`; first list stays | IT-12-7 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B04) |
| 1.1.0 | 2026-11-11 | Registration entry point named (`register_hooks`, set once, unset fails closed); spike decision | AZ-3681 |
