# Contract: operator identity operations and invites

**Component**: 01_identity
**Producer task**: AZ-3688 (E-15/c08)
**Consumer tasks**: E-22/c08 (CLI `admin create|reset|setup-token|invite`), E-21/c12 (admin invite routes), AZ-3654 (first-run pipeline), E-15/c12 (AT-01-1)
**Version**: 1.2.0
**Status**: frozen (first implementation: AZ-3688)
**Last Updated**: 2026-11-13

## Purpose

What the setup token, invites and operator operations do, so the CLI (12) and the admin routes (11) can call them without reading identity's internals. Trait signatures live in `contracts` (`identity/invite.rs`); the implementation is `sunnycloud_identity::IdentityInvites`, built by 12 with `IdentityInvites::new(db, clock, &IdentityConfig, AuditWriter)`.

## Shape

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-----------|-----------------|-----------|
| `OperatorIdentityOps::ensure_setup_token` | `() -> Option<RawToken>` | `Unexpected` only; `None` when any account exists | async, system scope |
| `OperatorIdentityOps::create_admin_invite` | `() -> InviteToken` | `Unexpected` only | async, system scope |
| `OperatorIdentityOps::reset_admin` | `(AccountId) -> InviteToken` | `OperatorIdentityError::NotFound` (unknown or `deleting` account), `OperatorIdentityError::NotAnAdmin`; none for the only active admin (G-12-3) | async, system scope |
| `InviteService::create_invite` | `(admin AccountId, note) -> InviteToken` | `Unexpected` only | async, system scope |
| `InviteService::list` | `(admin AccountId) -> [InviteInfo]` (no token) | `Unexpected` only | async, system scope |
| `InviteService::revoke` | `(admin AccountId, invite id) -> ()` | `InviteError::NotFound` for an unknown, already revoked or redeemed invite alike (11: 404 `not_found`) | async, system scope |

Redemption happens at sign-in (`SignInService::complete`) when an unknown identity presented a token at `begin` (`invite_token` or `setup_token`, same field): success creates or re-binds an account; any unusable token gives `InvitationRequired` and writes nothing.

| Token | Valid while | Redemption result |
|-------|-------------|-------------------|
| setup | 0 accounts exist, not redeemed, not expired (`setup_token_ttl` 7 d) | first account, role admin, hooks run |
| member invite | not redeemed, not revoked, not expired (`invite_ttl` 7 d) | new account, role `user`, hooks run |
| operator admin invite | same | new account, role admin, hooks run |
| re-bind invite (`target_account_id`) | same | no new account, no hook; identity bound to the target, its previous identities removed; role, state and data kept |

`list` returns member and operator admin invites (newest first, redeemed ones with `redeemed_at`, note `""` when none); setup tokens and re-bind invites are never listed and cannot be revoked through `InviteService`. `revoke` deletes the open row (the schema has no `revoked_at`, so a revoked invite leaves the list; redeemed ones stay). `list` ignores `admin_account`: every admin sees all invites.

## Invariants

- Every token is returned in plaintext exactly once, by the call that creates it; only its SHA-256 is stored; `Debug` of the token is redacted and identity never logs it. Printing the setup token is the caller's (AZ-3654).
- At most one setup token is open at a time; `ensure_setup_token` replaces the open one (expired or not), so the earlier plaintext stops working.
- `ensure_setup_token` never issues a token while an account exists; the CLI maps `None` to its "setup not available" outcome (FT-P-78: exit code 2, nothing printed). A setup token issued before the first account stops redeeming once any account exists.
- Every token redeems at most once, also under concurrent sign-ins (AC-75): the conditional `UPDATE ... WHERE redeemed_at IS NULL AND expires_at > now` decides; concurrent setup redemptions leave one admin.
- `reset_admin`, in one transaction, before it returns: revokes the admin's sessions and device tokens (reason `admin`; owner decision 2026-11-13), denies approved device grants not yet collected, deletes unexchanged mobile login codes, removes the account's identities (so nobody can sign in as that admin until the invite is redeemed), deletes earlier open re-bind invites of the account (only the newest reset works) and stores the new re-bind invite (`grants_role = admin`, no creator).
- The mandatory audit events (owner decision 2026-11-13) are written in the transaction of the change; a failed write fails the call and changes nothing: `admin_reset` (kind `admin_action`, no actor, subject the account, `invite_id`), `session_revoked` (reason `admin`, when a session was live), `identity_rebound` at re-bind redemption, `account_created` (with the granted role) at creating redemption.
- Identity has no `LastAdmin` error; promote and demote go through 07.
- `reset_admin` of the only active admin is allowed (owner decision 2026-11-11, G-12-3): the account keeps role admin and state active; no last-admin rule applies.

## Non-Goals

- Not covered: printing the token and CLI exit codes (12); HTTP status codes beyond `InviteError::NotFound` → 404 (11).
- Not covered: promote, demote, the last-admin rule (07).

## Versioning Rules

- **Breaking changes** (a token returned more than once, a changed validity rule, `reset_admin` creating a new account, re-bind calling a hook) require a new major version.
- **Non-breaking additions** (a new optional invite field such as a role in `InviteInfo`) require a minor version bump.

## Test Cases

| Case | Input | Expected | Test |
|------|-------|----------|------|
| setup-once | empty DB; none, wrong, right, right again | 0, 0, 1 admin; reuse `InvitationRequired` | `setup_token::the_setup_token_creates_one_admin_once_and_nothing_without_or_with_a_wrong_token` (IT-01-7) |
| setup-replaced | `ensure_setup_token` twice | first plaintext refused | `setup_token::a_second_ensure_replaces_the_first_token_and_only_the_newest_redeems` (FT-P-78) |
| setup-expired | clock past `setup_token_ttl` | `InvitationRequired`, nothing written | `setup_token::an_expired_setup_token_gives_invitation_required_and_creates_nothing` |
| setup-with-account | account exists | `None`, nothing stored | `setup_token::ensure_returns_nothing_and_stores_nothing_once_an_account_exists` |
| setup-race | 8 concurrent redemptions | one admin | `setup_token::concurrent_setup_redemptions_leave_exactly_one_admin` |
| invite-single-use | member invite used twice | one account; second refused; list has no token | `invites::a_member_invite_is_returned_once_listed_without_its_token_and_redeems_once` (FT-N-21) |
| revoke | revoked invite redeemed | `InvitationRequired` | `invites::a_revoked_invite_gives_invitation_required` |
| revoke-not-found | unknown, revoked, redeemed | `InviteError::NotFound` alike, nothing changed; 11 answers 404 | `invites::revoke_answers_not_found_alike_for_an_unknown_a_revoked_and_a_redeemed_invite`; 11 `identity::admin::revoking_an_unknown_revoked_or_redeemed_invite_is_404` |
| admin-invite | `create_admin_invite` redeemed | second admin; no creator | `invites::an_operator_admin_invite_creates_a_second_admin_and_has_no_creator` |
| rebind | `reset_admin(X)`, identity Y redeems | account count unchanged; X identities = {Y}; role, nodes kept | `admin_reset::a_reset_revokes_at_once_and_its_invite_rebinds_the_account_to_a_new_identity_keeping_role_and_data` (IT-01-9) |
| rebind-old-identity | old identity after reset; earlier reset invite | both `InvitationRequired` | `admin_reset::after_a_reset_the_old_identity_no_longer_signs_in_and_an_earlier_reset_invite_stops_working` |
| sole-admin-reset | X is the only active admin | invite returned; X stays admin and active; credentials revoked | `admin_reset::the_only_active_admin_can_be_reset_and_stays_an_active_admin` (IT-12-2) |
| reset-refused | unknown or member account | `NotFound` / `NotAnAdmin`, nothing changed | `admin_reset::a_reset_of_an_unknown_or_a_member_account_is_refused_and_changes_nothing` |
| reset-audit-fails | audit insert fails | call fails, nothing changed | `admin_reset::a_failed_admin_reset_audit_write_fails_the_reset_and_changes_nothing` |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B04) |
| 1.1.0 | 2026-11-11 | G-12-3 recorded: reset of the only active admin is allowed, no new error | AZ-3681 |
| 1.2.0 | 2026-11-13 | Frozen with the first implementation: `revoke` answers `InviteError::NotFound` for unknown, revoked or redeemed alike (owner); `reset_admin` also revokes device tokens (owner), denies uncollected grants, deletes login codes, removes identities at once and supersedes earlier re-bind invites; `OperatorIdentityError::{NotFound, NotAnAdmin}`; list scope; mandatory `admin_reset` and `identity_rebound` events | AZ-3688 |
