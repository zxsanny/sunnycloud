# Email rule, invite redemption and the account-creation transaction over the lifecycle hooks

**Task**: AZ-3685_invite_redemption_account_creation
**Name**: Invite redemption and account creation
**Description**: Create an account from a single-use invite in one short transaction that also runs every registered lifecycle hook, never use an unverified email, and leave exactly one account when redemptions race
**Complexity**: 5 points
**Dependencies**: AZ-3681_identity_hooks_admin_reset_spike, AZ-3684_provider_sign_in_callbacks, AZ-3768_contracts_part1, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3751_quota_admin_lifecycle, AZ-3778_feed_horizon_lifecycle, AZ-3717_tree_lifecycle_blob_cleanup
**Component**: 01 identity
**Tracker**: AZ-3685
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c05

## Problem

Registration is controlled: an unknown identity becomes an account only by redeeming a single-use invite (AC-23, RESTRICT-18). The new account needs rows owned by other components (root node, account feed, quota) created in the same transaction, or a lost race leaves orphans. Two browsers redeeming one invite at once must yield one account (AC-75). A provider email the provider does not verify must never create or link an account (AC-55).

## Outcome

- `SignInService::complete` registers an unknown identity that carries a valid invite: account, identity, every registered hook's `initialize`, then the redemption, all in one short transaction.
- Any failure in that transaction rolls back every row and leaves the invite redeemable.
- The email rule of 01 §5 holds.
- The behavioural contract `_docs/02_document/contracts/01_identity/account_creation_hooks.md`, which the hook implementers (E-11, E-12, E-13, E-16) and the composition root (E-22) read.

## Scope

### Included
- Registration branch of `complete`: in one transaction insert the account (role from the invite's `grants_role`, state active, display name from the provider profile) and the identity, call `initialize(tx, account_id)` of every hook in the registered list in registration order, then redeem with the single conditional update (`redeemed_at IS NULL AND expires_at > now`, time from the injectable clock).
- The loser of a redemption race gets `InvitationRequired` and leaves no row.
- The hook list arrives through the entry point decided in E-15/c01.
- Email rule: an email is read only when `email_allowlist` is configured, only if verified (OIDC `email_verified`; GitHub per-address `verified`), only at invite redemption; an unverified address is treated as no email; email is never a lookup key.
- The contract file.

### Excluded
- Setup-token rules, operator invites and re-bind invites (E-15/c08, which reuse this redemption).
- Creating invites (E-15/c08); provider linking (E-15/c14); audit events (E-15/c11).
- The hook implementations (E-11/c03, E-12/c06, E-13/c13) and registering the list (E-22/c03).

## Acceptance Criteria

**AC-1: An invite creates one complete account**
Given a valid member invite (fixture row) and the real hooks of 02, 05 and 07a registered
When an unknown identity completes sign-in with it
Then exactly one account (role from the invite), one identity, one root node, one account feed and one quota row exist, the invite is redeemed once, and a session is returned

**AC-2: No invite, no account (AC-23)**
Given an unknown identity
When it completes sign-in without an invite, with an unknown invite, or with an expired one (clock past `expires_at`)
Then the result is `InvitationRequired` and no row is created in any table

**AC-3: Unverified email never creates or links (AC-55)**
Given an allowlist containing `victim@example.test`, an existing account for the victim, and mock-idp returning that email as OIDC `email_verified=false`, as GitHub `verified=false`, and as a relay address
When each completes sign-in without an invite, and then a verified variant completes once without and once with a valid invite
Then the 3 unverified variants and the verified one without invite get `InvitationRequired` with account and identity counts unchanged; the verified one with an invite gets a new account; the email is never used to find an account

**AC-75: One invite, concurrent redemptions, one account (component part, R8)**
Given 200 iterations, each with a fresh invite and 2, then 16, distinct provider identities on separate pool connections released by a barrier
When all call `complete` with the same invite
Then in every iteration exactly one call returns a session and the rest `InvitationRequired`; the losers leave no account, identity, root node, feed or quota row; 0 iterations end with 2 accounts

**AC-4: A failing hook rolls everything back**
Given the real hooks plus a fault-injecting hook (test double of the `AccountLifecycleHook` port) that fails on its first call
When an invite is redeemed, then redeemed again without the fault
Then the first attempt returns a typed failure with accounts, identities, root node, feed and quota unchanged and the invite still redeemable; the second attempt creates all rows in one transaction

## Non-Functional Requirements

**Reliability**
- The account-creation transaction holds no external I/O: the provider exchange is finished before it opens (short transaction rule).

**Performance**
- Callback to session within 1 s at p95 with a local provider, excluding provider time (AZ-3664 NFR; measured there).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Email extraction per provider kind and flag | Unverified or relay address yields "no email" |
| AC-3 | Allowlist off | No email is read at all |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Invite, real hooks 02/05/07a | Unknown identity signs in | One account with all hook rows; invite redeemed | FT-P-65 |
| AC-2 | No, unknown, expired invite | `complete` | `InvitationRequired`; 0 rows | FT-N-20, FT-N-21 |
| AC-3 | Victim email unverified x3, verified x2 | IT-01-6 | Counts unchanged except verified with invite | FT-N-22, NFT-SEC-02 |
| AC-75 | 200 iterations, 2 and 16 racers | IT-01-8 | One winner each; losers leave no rows | NFT-SEC-48 |
| AC-4 | Fault-injecting hook | IT-01-17 | Full rollback; invite redeemable; retry succeeds | |

## Constraints

- Identity calls only `initialize`; it never calls `erase_batch` and imports no hook implementer (ADR-015).
- The cross-component outcome of AC-75 through the real server and hook list is owned by the pipeline task AZ-3664 (E-22/p02); this task owns the transaction and its concurrency suite.
- Tests run the real hooks of 02, 05 and 07a on PostgreSQL 18; only the fault-injecting hook is a test double.

## Risks & Mitigation

**Risk 1: A hook does slow work inside the transaction**
- *Risk*: a hook's `initialize` holds locks or does I/O and stretches the transaction.
- *Mitigation*: the contract forbids I/O outside the given transaction; IT-01-8 at 16 racers would show lock waits.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| What the allowlist does at redemption is not stated: is a verified email that is not on `email_allowlist` refused, or only not recorded? AC-23 says only that an email is matched at invite redemption | Product owner | resolved 2026-11-13: with the allowlist set, a verified email not on it refuses the redemption (`InvitationRequired`, no row) | Medium |
| `accounts.display_name` is NOT NULL but an OIDC provider may send no name claim; the fallback is not stated | E-15 owner | resolved 2026-11-13: the provider's name claim, else its username (GitHub `login`, OIDC `preferred_username`), else `User` | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/01_identity/account_creation_hooks.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
