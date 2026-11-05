# Contract: operator identity operations and invites

**Component**: 01_identity
**Producer task**: B04-08_setup_token_invites_operator_ops (E-15/c08)
**Consumer tasks**: E-22/c08 (CLI `admin create|reset|setup-token|invite`), E-21/c12 (admin invite routes), AZ-3654 (first-run pipeline), E-15/c12 (AT-01-1)
**Version**: 1.0.0
**Status**: draft (freezes when E-15/c01 records the G-12-3 answer)
**Last Updated**: 2026-11-04

## Purpose

What the setup token, invites and operator operations do, so the CLI (12) and the admin routes (11) can call them without reading identity's internals. Trait signatures live in `contracts` (E-00/c03).

## Shape

| Name | Signature (behavioural) | Throws / Errors | Blocking? |
|------|-----------|-----------------|-----------|
| `OperatorIdentityOps::ensure_setup_token` | `() -> Option<Plaintext>` | none; `None` when any account exists | async, system scope |
| `OperatorIdentityOps::create_admin_invite` | `() -> Plaintext` | DB errors only | async, system scope |
| `OperatorIdentityOps::reset_admin` | `(AccountId) -> Plaintext` | sole-admin outcome per G-12-3; the error for an unknown or non-admin account is not named yet (see Non-Goals) | async, system scope |
| `InviteService::create_invite` | `(admin AccountId, note) -> Plaintext` | DB errors only | async |
| `InviteService::list` | `() -> [InviteInfo]` (no token) | none | async |
| `InviteService::revoke` | `(invite id) -> ()` | see Non-Goals | async |

Redemption outcomes seen at sign-in (`SignInService::complete`): success creates or re-binds an account; any unusable token gives `InvitationRequired`.

| Token | Valid while | Redemption result |
|-------|-------------|-------------------|
| setup | 0 accounts exist, not redeemed, not expired (`setup_token_ttl` 7 d) | first account, role admin |
| member invite | not redeemed, not revoked, not expired (`invite_ttl` 7 d) | new account, role `user` |
| operator admin invite | same | new account, role admin |
| re-bind invite (`target_account_id`) | same | no new account; identity bound to the target, its previous identities removed; role and data kept |

## Invariants

- Every token is returned in plaintext exactly once, by the call that creates it; only its SHA-256 is stored.
- At most one setup token is open at a time; `ensure_setup_token` replaces the open one, so the earlier plaintext stops working.
- `ensure_setup_token` never issues a token while an account exists; the CLI maps `None` to its "setup not available" outcome (FT-P-78: exit code 2, nothing printed).
- Every token redeems at most once, also under concurrent sign-ins (AC-75).
- `reset_admin` revokes the target admin's sessions before it returns.
- Identity has no `LastAdmin` error; promote and demote go through 07.

## Non-Goals

- Not covered: printing the token and CLI exit codes (12); HTTP status codes (11).
- Not covered: promote, demote, the last-admin rule (07).
- Open: the result of `revoke` for an unknown or already redeemed invite; the error of `reset_admin` for an unknown or non-admin account; whether `reset_admin` also revokes device tokens.

## Versioning Rules

- **Breaking changes** (a token returned more than once, a changed validity rule, `reset_admin` creating a new account) require a new major version.
- **Non-breaking additions** (a new optional invite field such as a note filter) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| setup-once | empty DB, token used twice | 1 admin; second use `InvitationRequired` | IT-01-7 |
| setup-replaced | `ensure_setup_token` twice | first plaintext refused | IT-01-7, FT-P-78 |
| setup-with-account | account exists | `None` | IT-01-7, FT-P-78 |
| invite-single-use | member invite used twice | one account; second refused | FT-N-21 |
| rebind | `reset_admin(X)`, identity Y redeems | account count unchanged; X identities = {Y} | IT-01-9 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B04) |
