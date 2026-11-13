# Setup token, InviteService, OperatorIdentityOps and admin re-bind

**Task**: AZ-3688_setup_token_invites_operator_ops
**Name**: Setup token, invites and operator identity operations
**Description**: Bootstrap the first admin with a one-time setup token that works only while no account exists, let admins and the operator issue single-use invites, and let the operator re-bind an admin account to a new identity
**Complexity**: 5 points
**Dependencies**: AZ-3681_identity_hooks_admin_reset_spike, AZ-3685_invite_redemption_account_creation, AZ-3686_session_refresh_rotation, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base
**Component**: 01 identity
**Tracker**: AZ-3688
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/account_creation_hooks.md` — re-bind redemption creates no account and calls no hook (produced by E-15/c05).

## Problem

A fresh install has no account and no admin (AC-23, RESTRICT-18). The first admin is created with a setup token printed once at start; later users come only through single-use invites. An operator who lost the token, or whose admin lost access to their provider account, must recover from the command line without editing the database.

## Outcome

- `OperatorIdentityOps`: `ensure_setup_token`, `create_admin_invite`, `reset_admin`.
- `InviteService`: `create_invite`, `list`, `revoke`.
- Setup-token and re-bind redemption added to the redemption of E-15/c05.
- The behavioural contract `_docs/02_document/contracts/01_identity/operator_identity_ops.md`, which E-21 (admin invite routes) and E-22 (CLI commands) read.

## Scope

### Included
- `ensure_setup_token()`: only while `accounts` is empty; creates the setup token (`setup_token_ttl`, 7 d), replacing any open one; returns the plaintext once; returns nothing when an account exists. Only the hash is stored.
- Setup redemption: valid only while `accounts` is empty; creates the first account with role admin through the account-creation transaction of E-15/c05.
- `create_invite(admin_account, note)`: member invite (`invite_ttl`, 7 d), plaintext returned once; `list` without tokens; `revoke` makes it unredeemable.
- `create_admin_invite()`: operator invite with `grants_role = admin` and no creator.
- `reset_admin(account)`: revokes that admin's sessions at once and issues an operator invite with `target_account_id` = that account.
- Re-bind redemption: no new account and no hook call; in the same transaction the signing-in identity is bound to the target account and its previous identities are removed; role, nodes and files stay.
- The sole-admin rule decided by E-15/c01 (G-12-3).
- The contract file.

### Excluded
- Printing the token at `serve` start (the `serve` callsite is owned by AZ-3654, E-22/p01) and the CLI commands with their exit codes (E-22/c08).
- Admin invite routes (E-21/c12) and the web admin pages (E-26/c16).
- Promote and demote and the last-admin rule (07, E-17/c04).

## Acceptance Criteria

**AC-1: Setup token bootstraps the first admin once (AC-23)**
Given an empty database and the token returned by `ensure_setup_token`
When sign-ins run without a token, with a wrong token, with the right token, and with the right token again
Then without and with the wrong token the result is `InvitationRequired` with 0 accounts; the right token creates 1 account with role admin and redeems the token; the second use is `InvitationRequired` and accounts stay 1

**AC-2: Setup token replacement and refusal (FT-P-78, port part)**
Given an empty database
When `ensure_setup_token` is called twice, the first plaintext is presented, a fresh one expires (clock past `setup_token_ttl`), and `ensure_setup_token` is called after an account exists
Then the second call replaced the first (the first plaintext gives `InvitationRequired`); the expired token gives `InvitationRequired`; the call with an account present returns nothing and stores nothing

**AC-3: Member invites are single use (AC-23, RESTRICT-18)**
Given an admin account
When the admin creates an invite, lists invites, an identity redeems it, another identity tries it again, and a second invite is revoked and then tried
Then the plaintext is returned once and only its hash stored; the list holds no token; the first redemption creates a member account; the reuse and the revoked invite give `InvitationRequired`

**AC-4: Operator admin invite**
Given an install with one admin
When `create_admin_invite` runs and a new identity redeems the invite
Then a second account with role admin exists, and the invite row has no creator

**AC-5: Admin re-bind keeps the account (IT-01-9)**
Given admin X with two sessions and a node tree
When `reset_admin(X)` runs and a new identity Y redeems the returned invite, then the invite is tried again
Then X's sessions are revoked at once; the invite has `target_account_id = X` and no creator; after redemption the account count is unchanged, X's identities are exactly {Y}, X keeps role admin and its root node and files; the second try gives `InvitationRequired`

**AC-6: Sole-admin reset follows the recorded decision (G-12-3)**
Given X is the only active admin
When `reset_admin(X)` runs
Then the outcome is the one recorded by E-15/c01 (allowed as in AC-5, or refused with the agreed error and no change)

## Non-Functional Requirements

**Reliability**
- Setup and re-bind redemption run inside the short account-creation transaction (E-15/c05); concurrent setup redemptions leave one admin (FT-N-21, AZ-3654 AC-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Setup availability rule | Available only with zero accounts |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty DB | IT-01-7 main path | 0 accounts without/wrong token; 1 admin; reuse refused | FT-P-64, FT-N-20, NFT-SEC-01 |
| AC-2 | Empty DB, manual clock | IT-01-7 replace, ttl, accounts-present | Old refused; expired refused; nothing issued with an account | FT-P-78 |
| AC-3 | One admin | Invite lifecycle | Plaintext once; list without token; single use; revoked refused | FT-P-65, FT-N-21 |
| AC-4 | One admin | `create_admin_invite` redeemed | Second admin; no creator | FT-P-65 |
| AC-5 | Admin X with sessions and tree | IT-01-9 | Sessions revoked; re-bound to Y; role and data kept | |
| AC-6 | X sole admin | `reset_admin(X)` | As recorded by E-15/c01 | IT-12-2, AT-12-2 |

## Constraints

- Setup token and invites are rows of `invites` (`kind` = `setup` or `invite`, data model D2); only hashes are stored.
- The setup token is printed once to stdout by the caller (documented exception to "no secrets in logs"); this service only returns it.

## Risks & Mitigation

**Risk 1: Leaked setup token**
- *Risk*: the printed token reaches a shipped log.
- *Mitigation*: replacement by `ensure_setup_token` (AC-2) and the zero-accounts rule; printing rules are AZ-3654's.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `revoke` of an unknown or already redeemed invite: result not stated in 01 | E-15 owner | resolved 2026-11-13 (owner): `NotFound` for unknown, revoked or redeemed alike, so revoke reveals nothing | Low |
| `reset_admin` revokes sessions only; whether the admin's device tokens are also revoked is not stated | E-15 owner | resolved 2026-11-13 (owner): `reset_admin` also revokes the admin's device tokens | Low |
| G-12-3 owner answer: reset of the only active admin is allowed (2026-11-11, AZ-3681) | Product owner | resolved | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/01_identity/operator_identity_ops.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
