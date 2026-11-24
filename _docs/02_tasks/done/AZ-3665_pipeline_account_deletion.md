# Account deletion pipeline (F13)

**Task**: AZ-3665_pipeline_account_deletion
**Name**: Account deletion pipeline
**Description**: A confirmed account delete ends access at once, and the scheduled purger drives every lifecycle hook and the GC until nothing of the account is left, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3784_sc_audit, AZ-3801_crate_skeleton_accept_loop, AZ-3828_identity_admin_handlers, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3795_user_seed_audit_commands, AZ-3818_last_admin_guard, AZ-3819_account_view_deletion_request, AZ-3820_deletion_purger, AZ-3690_account_admin_and_guard, AZ-3717_tree_lifecycle_blob_cleanup, AZ-3757_transfer_erase_and_crash_cases, AZ-3777_feed_long_poll, AZ-3778_feed_horizon_lifecycle, AZ-3702_share_account_erase_hook, AZ-3751_quota_admin_lifecycle, AZ-3845_media_janitors_erase_hook, AZ-3730_gc_sweeper, AZ-3925_vault_erase_hook, AZ-3663_pipeline_gc_retention_restore, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3665
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p12

## Problem

Account erase is the widest join in the server: 07's `DeletionPurger` walks the `AccountLifecycleHook` list of 06, 20, 02, 03, 08, then 05 and 07a in the order 12 registers it, 01 revokes credentials first and finalizes last, and 04's GC unlinks the files. E-17/c11 checks the hook chain in process; no planned task proves through the real server that a delete ends live requests within 5 s and leaves no row or blob file behind.

## Outcome

- A confirmed delete through the real `serve` process revokes every credential at once, refuses further commits, and the scheduled purger removes all account data within 15 minutes.
- Every component operation of flow F13 has a production caller, with the hook list in the documented order.
- At least one integration test runs F13 through the real binaries over HTTP.

## Scope

### Included
- Flow order and seams (`system-flows.md` F13 steps 1-3):
  1. Account route (session only, explicit confirmation) or `admin user delete --yes` → `AccountDeletionService::request` → `AccountAdmin::begin_deletion`: state `deleting`, sessions and device tokens revoked → held long-polls end via the revalidator → `AccountGuard::ensure_active` refuses commits.
  2. Scheduler (12) → `DeletionPurger::run_cycle` → `AccountLifecycleHook::erase_batch` for 06, 20 (v1.2 builds), 02, 03, 08, then 05, 07a, in short batches until every hook reports `remaining = 0`; 02 and 08 queue blobs with `Due::Now`.
  3. `GcSweeper` unlinks the files → `AccountAdmin::finalize_deletion` deletes identities, sessions, device tokens, grants, login codes and the account row → audit entry with ids only.
- The end-to-end integration test of the steps above.

### Excluded
- The purger's claim mechanism (E-17/c01 spike), last-admin internals (E-17/c04), vault rows before v1.2.

## Acceptance Criteria

AC-1..AC-5 are local to this task. No interaction risk of `interaction-risks.md` is owned here (F13 risks are covered by AC-27, AC-30, AC-21).

### Wiring and flow criteria

**AC-1: Access ends at once**
Given an account with a held long-poll, an active device token and an upload in progress
When the user confirms the delete through the account route
Then the long-poll ends with 401 within 5 s, the device token gets 401, and the pending commit is refused as account not active

**AC-2: Everything is purged**
Given that account holding files with thumbnails, a share link, an open upload session, staged files and queued jobs
When the scheduler runs the purger and GC cycles
Then every hook reports `remaining = 0`, no row of the account remains in any table, its blob files are unlinked, the account row is gone, and one audit entry with ids only records the deletion

**AC-3: Purge resumes after a crash**
Given a purge in progress
When the server is killed and restarted
Then the purger resumes from the `deleting` state and ends with the same clean result

**AC-4: Guards**
Given the last admin, and a device token of another account
When the last admin requests deletion, and the device token calls the delete route
Then the first is refused with a last-admin error and the second gets 403; a later sign-in with the deleted identity is an unknown identity and needs an invite

**AC-5: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `AccountDeletionService::request` (route and `admin user delete`), a scheduler registration of `DeletionPurger`, the hook list passed by 12 with every implementation (02, 03, 05, 06, 07a, 08; 20 in v1.2 builds) in the documented order, `AccountAdmin::{begin_deletion, finalize_deletion}`, and `GcSweeper` removing the files

## Non-Functional Requirements

**Performance**
- All data removed within 15 minutes for a large account (AC-27); this task uses a small account, the large case is E-17/c10.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Static caller check over non-test sources | Each named call has a production caller; hook list order matches the spec |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Held poll, device token, upload in progress | Confirm delete | 401 within 5 s, token 401, commit refused | AC-21 |
| AC-2 | Account with files, thumbnails, share, sessions, staged, jobs | Purger and GC cycles | No rows, no files, audit ids only | AC-27, AC-30 |
| AC-3 | Purge in progress | Kill, restart | Resumes, clean result | |
| AC-4 | Last admin; foreign device token | Delete | Last-admin refusal; 403; identity unknown later | AC-57 |

## Constraints

- Owner: component 12 (`src/server/`): hook-list and purger registration and the flow's end-to-end tests among 12's integration tests. No SQL or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume over HTTP; no sunnycloud component is faked.
- Downstream: E-31/c25 (FT-P-19) and E-31/c36 should depend on this task.

## Risks & Mitigation

**Risk 1: A hook missing from the list**
- *Risk*: a new component's hook is never registered, so its rows survive deletion.
- *Mitigation*: AC-2 checks every table of the data model for the account id.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Purger claim mechanism (G-07-2): per-step `FOR UPDATE SKIP LOCKED` claim via the proposed `AccountAdmin::lock_deleting`, no lease column (resolved 2026-11-07, AZ-3814) | E-17 | resolved | Low |
| KNOWN RISK (G-07-2): a hook on its own connection that inserts a row with a foreign key to `accounts` takes `FOR KEY SHARE` on the `deleting` row, which the purger's `FOR UPDATE` blocks (self-deadlock); `FOR NO KEY UPDATE` avoids it. Owner asked; E-17/c06 must confirm no deadlock in IT-07-9 | E-17 / E-15 | open | Medium |
| How the v1.2 vault hook is left out of v1.0/v1.1 builds is open (module-layout exception 6) | E-22, E-30 | open | Low |
| Resolved by owner decision 2026-11-05: 20's erase hook runs before 02's (`vaults.root_node_id` references 02's nodes); the documented order is 06, 20, 02, 03, 08, then 05, 07a | Owner | resolved | High |
