# Contract: account-creation hooks (`AccountLifecycleHook::initialize` as called by identity)

**Component**: 01_identity
**Producer task**: AZ-3685 (E-15/c05, invite redemption and account creation)
**Consumer tasks**: E-11/c03 (quota row), E-12/c06 (account feed), E-13/c13 (root node), E-16/c08 (sharing, no-op), E-22/c03 (registers the list), E-15/c08 (re-bind and setup redemption)
**Version**: 1.2.0
**Status**: frozen (first implementation: AZ-3685)
**Last Updated**: 2026-11-13

## Purpose

Identity is the only caller of `AccountLifecycleHook::initialize`. This file states when and how it calls it, and what a hook implementer must and must not do. The trait signature lives in `contracts` (`registries.rs`); `erase_batch` is driven by 07 and is not covered here.

## Shape

| Name | Signature | Errors | Blocking? |
|------|-----------|--------|-----------|
| `initialize` | `async fn initialize(&self, tx: &mut DbTx, account: Uuid) -> Result<(), LifecycleError>` | any error aborts the account creation | async, inside identity's transaction |
| registration | `IdentitySignIn::register_hooks(&self, hooks: Vec<Arc<dyn AccountLifecycleHook>>) -> Result<(), HooksAlreadyRegistered>` (inherent method of `sunnycloud_identity::IdentitySignIn`, not in `contracts`) | `HooksAlreadyRegistered` on a second call; the first list stays | called once by 12 at start, before the listeners accept requests |

Before the transaction (no database write, no provider I/O after this point: the provider exchange has finished):

0. The identity is unknown (no `identities` row for `(provider, provider_user_id)`); the sign-in state carried an invite token; the email rule passes (01 §5: with `email_allowlist` set, the provider's verified, non-relay email must be on the list, compared without regard to ASCII case, and an identity with no usable email is refused; with no allowlist no email is read); the hook list is registered. Otherwise: `InvitationRequired` (or the typed failure of an unset list) and nothing is written.

Call order inside the one account-creation transaction (system scope):

1. read the open invite by token hash (`kind = 'invite'`, `target_account_id IS NULL`, `redeemed_at IS NULL`, `expires_at > now`, now from the injected clock); none: roll back, `InvitationRequired`;
2. insert the `accounts` row (state `active`, role = the invite's `grants_role`, display name = the provider's name claim, else its username, else `User`) and the `identities` row;
3. call `initialize(tx, account_id)` of every registered hook, in registration order;
4. redeem the invite with the single conditional update `UPDATE invites SET redeemed_at = now, redeemed_by = account WHERE id = $1 AND redeemed_at IS NULL AND expires_at > now`; 0 rows: roll back, `InvitationRequired`;
5. commit. Any error in 1 to 4 rolls back the whole transaction.

The session (or mobile login code) is written after the commit, in its own transaction, as for any known identity.

## Invariants

- `initialize` is called exactly once per created account, inside the transaction that creates it, after the `accounts` and `identities` rows exist and before the invite is redeemed.
- The transaction runs in system scope, so a hook may read other accounts' rows it needs (05 reads the installation epoch from `account_feeds`); a hook writes only its own rows for `account`.
- A hook writes only through the given `tx`; it does not commit, roll back or open another transaction.
- A hook does no I/O outside that transaction (no network, no blob store) and returns quickly. Known exception, open for the E-12 owner: 05 writes the `.epoch` marker file through `EpochMarker::write` when it creates the first feed of an installation.
- If any hook returns an error, no row of any hook, no account, no identity and no redemption remains; the invite stays redeemable; `complete` returns `SignInError::Unexpected` (500), logged as `account creation hook failed` with no PII.
- When redemptions of one invite race, every racer but one finds 0 rows at step 4 (the update waits for the winner's commit and re-reads the row) and rolls back, so the losers' hooks leave no row and they get `InvitationRequired` (AC-75). Two tabs of the same provider identity that race are decided by the unique key of `identities`; the loser gets `InvitationRequired`.
- `initialize` is not called when no account is created: sign-in of a known identity, a re-bind invite (`target_account_id` set) and the setup token are not redeemed by this path (E-15/c08 owns them; until then they are `InvitationRequired`).
- Identity never imports a hook implementer; the list comes from the composition root (12).
- The list is set once. If account creation runs while it was never set, it fails closed: `SignInError::Unexpected`, logged as `account_hooks_not_registered`, nothing written, invite redeemable; an unset list is never treated as empty.
- Email is never a lookup key; it is stored on the new identity (`email_verified = true`) only when it was read under the email rule.

## Non-Goals

- Not covered: `erase_batch`, its order and the `DeletionPurger` (07).
- Not covered: what each hook creates (quota row, account feed with its epoch, root node); each implementer's spec owns that.

## Versioning Rules

- **Breaking changes** (moving the call outside the transaction, changing the order of steps 1 to 4, leaving system scope, calling `initialize` for re-bind) require a new major version.
- **Non-breaking additions** (a new hook in the list, a new optional error variant hooks may return) require a minor version bump.

## Test Cases

| Case | Input | Expected | Test |
|------|-------|----------|------|
| valid-creation | unknown identity with a valid invite, hooks 02/05/07a | account, identity, root node, feed, quota in one commit; invite redeemed by the account | `account_creation::an_invite_creates_one_account_with_its_identity_root_feed_and_quota_and_a_session` |
| no-invite | none, unknown, expired (before, at, during the sign-in) | `InvitationRequired`; no row | `account_creation::without_an_invite_with_an_unknown_one_or_an_expired_one_the_result_is_invitation_required_and_nothing_is_written` |
| failing-hook | one hook errors on its first call | no rows at all; invite still redeemable; retry succeeds | `account_creation::a_failing_hook_rolls_every_row_back_and_the_invite_stays_redeemable_for_the_retry` (IT-01-17) |
| concurrent-loser | 200 iterations of 2 and 16 racers on one invite | one account each; losers' hooks leave no rows | `account_creation_race::concurrent_redemptions_of_one_invite_create_exactly_one_account_and_the_losers_leave_no_row` (IT-01-8, NFT-SEC-48) |
| email-rule | unverified, relay, unlisted, missing email with allowlist set | `InvitationRequired`; no row; invite redeemable | `account_creation_email::*` (IT-01-6) |
| rebind | invite with `target_account_id` | no `initialize` call; no new account | IT-01-9 (E-15/c08) |
| unset-list | `register_hooks` never called, valid invite | typed failure; no rows; invite redeemable | `account_creation::account_creation_fails_closed_while_no_hook_list_is_registered` |
| second-registration | `register_hooks` twice | second call `HooksAlreadyRegistered`; first list stays | `account_creation::a_second_hook_registration_is_refused_and_the_first_list_stays` (IT-12-7) |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B04) |
| 1.1.0 | 2026-11-11 | Registration entry point named (`register_hooks`, set once, unset fails closed); spike decision | AZ-3681 |
| 1.2.0 | 2026-11-13 | Frozen with the first implementation: precondition step 0 (email rule, owner decisions 2026-11-13), invite read in step 1, system scope, concrete signatures, error mapping, same-identity race, setup/re-bind excluded until E-15/c08 | AZ-3685 |
