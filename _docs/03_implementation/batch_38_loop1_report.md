# Batch Report

**Batch**: 38
**Tasks**: AZ-3681_identity_hooks_admin_reset_spike (spike, documents only, no code)
**Date**: 2026-11-11

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3681_identity_hooks_admin_reset_spike | Done: both decisions written. AC-4 does not apply (the owner chose "allow", so no refusal error exists) | `01_identity/description.md`, `01_identity/tests.md` (IT-01-17 unblocked), `12_operator_cli/description.md`, `12_operator_cli/tests.md`, contracts `account_creation_hooks.md` 1.1.0, `operator_identity_ops.md` 1.1.0, `operator_command_surface.md`, `tests/spec-gaps.md` (two rows resolved), epics E-15, E-17, E-22, flagged-concern rows of AZ-3664, AZ-3688, AZ-3787, AZ-3794, AZ-3818 | none (spike) | Items 1-3 |

## Decisions

1. **Hook entry point (agent decision, owner may overturn).** `register_hooks(&self, hooks: Vec<Arc<dyn AccountLifecycleHook>>) -> Result<(), HooksAlreadyRegistered>` is an inherent, set-once method of the identity service, not a port in `contracts`. A constructor parameter cannot work for identity: 02, 05 and 07a take identity's `AccountGuard` or `PrincipalRevalidator` when they are built, and identity needs their hooks, so the two sides form a cycle. 07's `DeletionPurger` takes its list in the constructor because nothing it drives depends on it. 12 is the only caller, and it now registers the late lists before building `HttpServer`, so no listener accepts a request with an unset list. If account creation runs with no list ever set, it fails closed (typed failure, nothing written, invite redeemable): an unset list is never read as an empty one, so no account appears without its quota row, feed and root node. One transaction is unchanged (AC-75).
2. **G-12-3 (owner, 2026-11-11).** `admin reset` of the only active admin is allowed. The account keeps role admin and state active, so no `LastAdmin` rule applies and no error is added. Until the invite is redeemed nobody can sign in as that admin.

## Code Review Verdict: PASS

Documents only. Two rows of `tests/spec-gaps.md` resolved; no BLOCKED marker left on IT-01-17.

## Test Suite

Not run: no code changed (last full run in batch 37: 2762 passed, 0 failed, 9 ignored).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | `register_hooks` is inherent on the concrete identity service, so no `contracts` change is needed; if you want it on a port instead (for a test double of the whole service), say so | AC-1 | Documented as inherent; the implementer of E-15/c05 adds the method and `HooksAlreadyRegistered` | unclear |
| 2 | 12's start sequence moved "register the late lists" before "build `ApiServices` and `HttpServer`" (it was after) so the list is set before any request | AC-1 | `12_operator_cli/description.md` Start; IT-12-7 checks the order | unclear |
| 3 | Unset-list failure is a typed `Unexpected` failure (500), not a startup refusal: a startup check would need a readiness hook for a list that 12 always sets; the cost is one failed sign-up if 12 is ever mis-wired | AC-1 | Chosen as the simplest; IT-01-17 has a last step for it | unclear |

## Commit

`[AZ-3681] Identity hook registration entry point and sole-admin reset decisions` — Loop: 1

## Next Batch

AZ-3681 is done. Ready now: AZ-3893 (Windows only) and the other 30 decision spikes (AZ-3705, 3721, 3722, 3731, 3732, 3787, 3799, 3849-3851, 3867, 3881-3887, 3913, 3921, 3930, 3933, 3934, 3973, 3985-3988, 4004, 4005), which need an owner decision. Tasks unblocked by this spike and waiting on others: AZ-3685, AZ-3688, AZ-3794. 250 tasks left.
