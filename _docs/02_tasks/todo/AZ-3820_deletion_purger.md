# DeletionPurger: erase an account across all components

**Task**: AZ-3820_deletion_purger
**Name**: Deletion purger
**Description**: A scheduled janitor claims accounts in `deleting`, drives every registered erase hook in order in short batches until each reports nothing left, then finalizes the account and audits it, resuming by query after any failure or crash
**Complexity**: 5 points
**Dependencies**: AZ-3814_admin_gaps_spike, AZ-3816_admin_crate_skeleton, AZ-3819_account_view_deletion_request, AZ-3690_account_admin_and_guard, AZ-3702_share_account_erase_hook, AZ-3717_tree_lifecycle_blob_cleanup, AZ-3757_transfer_erase_and_crash_cases, AZ-3778_feed_horizon_lifecycle, AZ-3751_quota_admin_lifecycle, AZ-3730_gc_sweeper, AZ-3784_sc_audit, AZ-3786_sc_observability
**Component**: 07 admin
**Tracker**: AZ-3820
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/account_admin_and_guard.md` — `finalize_deletion` (E-15/c10).
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `Janitor`, `AccountLifecycleHook`, `EraseProgress` (E-00/c03).
- `_docs/02_document/contracts/07_admin/account_services.md` — the `deleting` state set by the request (E-17/c05).

## Problem

Deleting an account must remove its rows, files, shares and sessions within 15 minutes (AC-27), immediately rather than after retention (AC-30), and must survive crashes and overlapping runs. The data spans seven components; each exposes an erase hook, and nobody drives them yet. One long transaction is forbidden; a missing or misordered hook leaves rows behind silently.

## Outcome

- `DeletionPurger` is a `Janitor` (`name()`, `run_cycle(now)`) that claims `deleting` accounts as decided by E-17/c01, so two purgers never process one account at the same time and no claim spans a cycle's idle time.
- Per account it walks the hooks in registration order, calling `erase_batch(account, account_delete_batch_size)` until the hook reports `remaining = 0` or the cycle's time budget ends; the next hook starts only after the current one reports 0.
- When every hook reports 0: one transaction with `finalize_deletion` and the `account_deleted` audit entry; the metric `account_deletion_seconds` records request to finalize.
- A failed batch is logged at ERROR with account id and hook position (no row data) and retried at the next tick; a deletion older than `account_delete_alert_after` is logged at WARN.

## Scope

### Included
- Claim, hook walk, batch bound, time budget, finalize, audit, metric, alert, resume by query.
- The suite IT-07-9 to IT-07-13 with the hooks of 06, 02, 03, 05 and 07a.

### Excluded
- Hook implementations (their epics); building the list (E-22/c03) and scheduling (E-22/c06).
- The 08 hook in the chain (E-17/c11); the large-account timing (E-17/c10).

## Acceptance Criteria

**AC-1: Drains every hook in order, finalizes last (AC-27, AC-30)**
Given an account with 10,000 live files in 200 folders, 400 retained revisions, 50 shares, 3 upload sessions with staged rows and reservations, 120 feed rows and a quota row, batch size 50, and recording wrappers around the real hooks of 06, 02, 03, 05 and 07a
When `run_cycle` runs until the account is gone
Then no `erase_batch` call has a limit above 50, each hook is first called only after the previous one reported 0, `finalize_deletion` runs once and last, the table sweep counts 0 rows for the account, its blobs are queued with `Due::Now` and 0 blob files remain after 04's GC, `account_deleted` is audited once, and other accounts' row counts are unchanged

**AC-2: Resume after a failure at each position**
Given the AC-1 account at 500 files and a decorator that fails the first call at hook position k (each hook, then `finalize_deletion`), one round per k, plus a round where the cycle is dropped mid-batch and a fresh purger instance starts
When cycles continue
Then each failure is logged at ERROR with account id and position and no row data, the next cycle continues from the remaining data without errors from already-deleted rows, the final sweep counts 0 rows in every round, `account_deleted` is audited once, and the account stays `deleting` until the last step

**AC-3: Time budget and batch bound**
Given an account with 5,000 files, batch size 50 and a cycle budget of 300 ms
When the purge runs
Then more than one cycle is needed, every call has limit 50, no purger transaction is open longer than 1 s (sampled every 50 ms), and the end state equals AC-1

**AC-4: Two purgers overlap**
Given an account with 2,000 files and two purger instances looping on separate connections
When both run until done
Then the final sweep counts 0 rows, no error is unhandled, and exactly one `account_deleted` entry exists

**AC-5: Nothing to do**
Given 20 active accounts and no `deleting` account
When `run_cycle` runs 100 times
Then `name()` is a stable non-empty string, no table shows an insert, update or delete, and each cycle returns in under 100 ms

**AC-6: Alert and metric**
Given a deletion older than `account_delete_alert_after` (20 s in test) that is not finished
When a cycle runs
Then a WARN line with the account id is written; and when the account is finalized, `account_deletion_seconds` records the time from request to finalize

## Non-Functional Requirements

**Performance**
- Longest purger transaction: 2 s target, 10 s failure (PT-07-2 in E-17/c10).

**Reliability**
- Idempotent per step; restart-safe by query alone; no leader.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Hook walk with doubles reporting remaining counts | Next hook only after 0; finalize only when all 0 |
| AC-3 | Budget check with a manual clock | Stops at the budget; resumes next cycle |
| AC-2 | Error on one hook | Logged with position; no finalize that cycle |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | tree-small without media rows; hooks 06, 02, 03, 05, 07a | IT-07-9 | Order kept; 0 rows; 0 files; one audit | FT-P-19, NFT-SEC-27 |
| AC-2 | 500 files; failing decorator per position; dropped cycle | IT-07-10 | Resumes; 0 rows; one audit | NFT-RES-30 |
| AC-3 | 5,000 files; batch 50; budget 300 ms | IT-07-11 | Several cycles; no transaction over 1 s | |
| AC-4 | 2,000 files; two purgers | IT-07-12 | Clean state; one audit | |
| AC-5 | 20 active accounts | IT-07-13 | No writes; under 100 ms | |
| AC-6 | Unfinished deletion past the alert time | Log and metric check | WARN with id; duration recorded | |

## Runtime Completeness

- Named capability: account erasure across components (F13).
- Production code that must exist: the claim, the hook walk over the real hooks registered by the composition root, and the finalize transaction.
- Allowed test decorators: recording and fault-injecting wrappers that delegate to the real hooks (named in IT-07-9 to IT-07-11).
- Unacceptable substitutes: a purger that deletes rows itself, a fake hook list in production wiring, or one long transaction.

## Constraints

- The order is the registration order given by the composition root (06, 20 in v1.2 builds, 02, 03, 08, then 05, 07a); 07 does not reorder.
- Each `erase_batch` runs in the hook's own short transaction; the purger holds no transaction across hook calls.

## Risks & Mitigation

**Risk 1: Deletion time depends on GC throughput (R12)**
- *Risk*: the tree hook's remaining count includes blobs the GC has not removed yet.
- *Mitigation*: drain within a budget per cycle; GC `Due::Now`; measured in PT-07-2.

**Risk 2: A hook missing from the list**
- *Risk*: rows of an unregistered component survive.
- *Mitigation*: AC-1 sweeps every table with an `account_id` column.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The claim mechanism and the port that takes it are decided by E-17/c01; it may need an 01 port method or column (a `contracts` change) | E-17 / E-15 owners | open | Medium |
| The per-cycle time budget has no named setting in 07 §7 (IT-07-11 sets 300 ms "in config") | E-17 owner | open | Low |
| Resolved by owner decision 2026-11-05: 20's erase hook runs before 02's, because `vaults.root_node_id` references 02's nodes; the production order is 06, 20, 02, 03, 08, then 05, 07a | Owner | resolved | High |

## Contract

This task produces the behaviour sections of `_docs/02_document/contracts/07_admin/deletion_purger.md` (driving rules every hook implementer relies on).
Consumers MUST read that file — not this task spec — to discover the interface.
