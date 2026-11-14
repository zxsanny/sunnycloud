# Contract: DeletionPurger (how account erase hooks are driven)

**Component**: 07_admin
**Producer task**: E-17/c02 (construction, AZ-3816), E-17/c06 (behaviour, AZ-3820)
**Consumer tasks**: E-22/c03 (builds the hook list), E-22/c06 (schedules the janitor); hook implementers E-16/c08, E-13/c13, E-14/c12, E-18/c10, E-30/c05, E-12/c06, E-11/c03; in batch E-17/c09, E-17/c10, E-17/c11; pipeline AZ-3665
**Version**: 1.2.0
**Status**: draft (open point 3 below)
**Last Updated**: 2026-11-14

## Purpose

The rules by which 07 erases an account across components (F13, AC-27, AC-30). Hook implementers read this file to know how `AccountLifecycleHook::erase_batch` is called; the composition root reads it to build the purger. Trait signatures are in `contracts` part 1.

## Shape

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-------------------------|--------|-----------|
| `DeletionPurger::name` | `() -> &'static str` (`"deletion_purger"`) | none | sync |
| `DeletionPurger::run_cycle` | `(now) -> CycleStats{processed: accounts finalized}` | `Unexpected` only when the `deleting` listing fails (a failed step is logged and retried, not returned) | async; bounded by `account_delete_cycle_budget` |
| hook call (by the purger) | `erase_batch(account, account_delete_batch_size) -> EraseProgress{remaining}` | the hook's error: logged, retried next tick | async; the hook's own short transaction |
| finalize (by the purger) | one transaction: `AccountAdmin::lock_deleting`, `AccountAdmin::finalize_deletion(tx, account)` and audit `account_deleted` (subject `account`, no actor) | logged, retried next tick | async |

### Construction (composition root)

`DeletionPurger::new(accounts: Arc<dyn AccountAdmin>, hooks: Vec<Arc<dyn AccountLifecycleHook>>, config: AdminConfig, audit: AuditWriter, db: Db, clock: Arc<dyn Clock>)`, then `.with_metrics(&Metrics)` to register `account_deletion_seconds` (a process without a registry leaves it out). `db` runs the claim transactions; `clock` measures the cycle budget (monotonic) and the deletion time (wall). The list order is the call order. Production order: 06 sharing, 20 vault_service (v1.2 builds only), 02 tree, 03 transfer, 08 media_jobs, then 05 sync_feed and 07a quota. 20 runs before 02 because `vaults.root_node_id` references 02's nodes (owner decision 2026-11-05). The purger keeps the order it is given and never sorts it.

### Settings

`account_delete_batch_size` (5000; tests 50), `account_delete_interval` (5 s; scheduler interval), `account_delete_alert_after` (10 min; tests 20 s), `account_delete_deadline` (15 min), `account_delete_cycle_budget` (5 s; tests 300 ms; owner decision 2026-11-14). A zero budget or batch size refuses the start.

## Invariants

Obligations of the purger:
- It processes only accounts in state `deleting`. Claim (decided 2026-11-07, G-07-2): per step, no lease column. Each step (one hook batch, or the finalize) opens its own short transaction in the account's scope, calls `AccountAdmin::lock_deleting(tx, account) -> bool` (`SELECT ... FOR NO KEY UPDATE SKIP LOCKED` on the account's `deleting` row; `false` when another transaction holds the row or the account is not `deleting`, and the account is skipped for the rest of the cycle), runs one bounded hook batch and commits. No claim is held across a cycle's idle time or between steps. Because the hooks are restart-safe by query alone, two purgers alternating steps on one account is harmless. The lock mode leaves `FOR KEY SHARE` free, so a hook inserting a row that references the account on its own connection does not wait for the claim (owner decision 2026-11-14; IT-07-9).
- A cycle lists the `deleting` accounts (oldest first, pages of `account_delete_batch_size`) and walks them in that order until `account_delete_cycle_budget` is spent, checked before every step on the clock's monotonic time; the rest wait for the next cycle.
- Per account it walks the hooks in passes. Within a pass it calls hook N + 1 only after hook N reported `remaining = 0`; it never passes a limit above `account_delete_batch_size`.
- It finalizes only after a pass, other than the walk's first, in which every hook reported 0 at its first call. A hook can report 0 in the call that erased its last rows, and a later hook can leave work an earlier one counts (03 queues the blobs of open uploads, which 02 counts until 04's GC removed them); the confirming pass catches both. Each walk therefore makes at least one extra pass of one call per hook.
- It finalizes only after every hook reported 0 in the same walk; finalize and the `account_deleted` audit entry are one transaction; `account_deleted` is written once per account.
- It holds no transaction across hook calls, except the claim transaction of the step that makes the call; that transaction lives for one bounded batch and ends with the step. It keeps no position in memory or in a table: after a crash or restart it starts the walk again from the first hook and relies on hooks reporting 0 quickly.
- A failed step is logged at ERROR (`account erase step failed`, field `account_id`, the error naming `hook position <k>` with k the 0-based index in the list, or `finalize (position <hook count>)`; never row data); the account is left for the cycle and retried at the next tick. A `deleting` account whose request is older than `account_delete_alert_after` at `now` is logged at WARN with its id on every cycle. `account_deletion_seconds` (histogram, no labels) records the time from `deletion_requested_at` to the finalize on the clock; a finalize is logged at INFO (`account deleted`).

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

1. Resolved 2026-11-07: per-step claim through `AccountAdmin::lock_deleting` (proposed `contracts` change task in E-15, a dependency of E-17/c06 (AZ-3820)); no column.
2. Resolved 2026-11-14: the time budget is `account_delete_cycle_budget` (5 s).
3. How the vault hook is left out of v1.0 builds is open (module-layout exception 6).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.0.1 | 2026-11-05 | Production order: 20 vault_service moved before 02 tree (owner decision 2026-11-05); the order rule itself is unchanged | decompose Step 4 |
| 1.1.0 | 2026-11-07 | Claim mechanism fixed: per-step claim through `AccountAdmin::lock_deleting` (proposed); claim transaction exception to the no-transaction-across-hook-calls rule (AZ-3814) | spike AZ-3814 |
| 1.2.0 | 2026-11-14 | Behaviour landed: constructor and `with_metrics`; claim lock mode `FOR NO KEY UPDATE SKIP LOCKED`; `account_delete_cycle_budget`; walk in passes with a confirming pass before the finalize; log lines and metric named. Hook obligations unchanged | AZ-3820 |
