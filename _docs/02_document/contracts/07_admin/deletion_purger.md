# Contract: DeletionPurger (how account erase hooks are driven)

**Component**: 07_admin
**Producer task**: E-17/c02 (construction), E-17/c06 (behaviour) — `_docs/02_tasks/todo/B05-02_admin_crate_skeleton.md`, `B05-06_deletion_purger.md` (renamed to their AZ keys at Step 4)
**Consumer tasks**: E-22/c03 (builds the hook list), E-22/c06 (schedules the janitor); hook implementers E-16/c08, E-13/c13, E-14/c12, E-18/c10, E-30/c05, E-12/c06, E-11/c03; in batch E-17/c09, E-17/c10, E-17/c11; pipeline AZ-3665
**Version**: 1.0.1
**Status**: draft (claim mechanism open, see Open points)
**Last Updated**: 2026-11-05

## Purpose

The rules by which 07 erases an account across components (F13, AC-27, AC-30). Hook implementers read this file to know how `AccountLifecycleHook::erase_batch` is called; the composition root reads it to build the purger. Trait signatures are in `contracts` part 1.

## Shape

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-------------------------|--------|-----------|
| `DeletionPurger::name` | `() -> &'static str` (stable, non-empty) | none | sync |
| `DeletionPurger::run_cycle` | `(now) -> CycleStats` | `Unexpected` (a failed batch is logged and retried, not returned) | async; bounded by the cycle time budget |
| hook call (by the purger) | `erase_batch(account, account_delete_batch_size) -> EraseProgress{remaining}` | the hook's error: logged, retried next tick | async; the hook's own short transaction |
| finalize (by the purger) | one transaction: `AccountAdmin::finalize_deletion(tx, account)` and audit `account_deleted` | `Unexpected` | async |

### Construction (composition root)

`DeletionPurger` is built with the ordered `AccountLifecycleHook` list, `AccountAdmin`, the admin settings, the audit writer, the clock and the database handle. The list order is the call order. Production order: 06 sharing, 20 vault_service (v1.2 builds only), 02 tree, 03 transfer, 08 media_jobs, then 05 sync_feed and 07a quota. 20 runs before 02 because `vaults.root_node_id` references 02's nodes (owner decision 2026-11-05). The purger keeps the order it is given and never sorts it.

### Settings

`account_delete_batch_size` (5000; tests 50), `account_delete_interval` (5 s; scheduler interval), `account_delete_alert_after` (10 min), `account_delete_deadline` (15 min). The per-cycle time budget is a setting whose name is open.

## Invariants

Obligations of the purger:
- It processes only accounts in state `deleting`, claimed so that two purgers never process one account at the same time and no claim is held across a cycle's idle time.
- Per account it calls hook N + 1 only after hook N reported `remaining = 0`; it never passes a limit above `account_delete_batch_size`.
- It finalizes only after every hook reported 0 in the same walk; finalize and the `account_deleted` audit entry are one transaction; `account_deleted` is written once per account.
- It holds no transaction across hook calls. It keeps no position in memory or in a table: after a crash or restart it starts the walk again from the first hook and relies on hooks reporting 0 quickly.
- A failed batch is logged at ERROR with account id and hook position (never row data) and retried at the next tick; a deletion older than `account_delete_alert_after` is logged at WARN; `account_deletion_seconds` records request to finalize.

Obligations of every hook implementer:
- `erase_batch` deletes at most `limit` rows of the account per call, in its own short transaction, and queues freed blobs with `GcQueue` `Due::Now` in that same transaction.
- It returns the count still to delete; 0 means nothing is left for this account in this component, including blobs not yet removed by GC where the component tracks them (02).
- It is idempotent: calling it again after 0, or for an account it never saw, returns 0 without error; a repeat after a partial failure does not fail on rows already gone.

## Non-Goals

- Not covered: what each hook deletes (each component's own contract).
- Not covered: scheduling and leader election (no leader; E-22/c06).

## Versioning Rules

- **Breaking changes** (order rule, the meaning of `remaining`, finalize timing, a new obligation for hooks) require a new major version and notice to every hook implementer and E-22.
- **Non-breaking additions** (a new metric, a new log field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| order-and-finalize | account across 06, 02, 03, (08), 05, 07a | next hook only after 0; finalize once, last; 0 rows | IT-07-9 |
| resume | failure at each position, dropped cycle | finishes; one `account_deleted` | IT-07-10 |
| budget | 5,000 files, batch 50, budget 300 ms | several cycles; no transaction over 1 s | IT-07-11 |
| overlap | two purgers | clean end; one audit entry | IT-07-12 |
| idle | no `deleting` account | no writes; cycle under 100 ms | IT-07-13 |

## Open points

1. Claim mechanism (per-step `SKIP LOCKED` claim or lease column) and the port that takes it: E-17/c01. It may need an 01 port method or column.
2. The time-budget setting has no name in 07 §7.
3. How the vault hook is left out of v1.0 builds is open (module-layout exception 6).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.0.1 | 2026-11-05 | Production order: 20 vault_service moved before 02 tree (owner decision 2026-11-05); the order rule itself is unchanged | decompose Step 4 |
