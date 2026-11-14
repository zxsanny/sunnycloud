# Batch Report

**Batch**: 67
**Tasks**: AZ-3927_vault_membership_wiring (done), AZ-3820_deletion_purger (done)
**Date**: 2026-11-14

Owner decisions 2026-11-14: AZ-3927 one task across 02, 03 and 06 with `vault_file_too_large` (413); AZ-3820 `lock_deleting` with `FOR NO KEY UPDATE SKIP LOCKED` and `account_delete_cycle_budget` 5 s. Two parallel workers on disjoint files. The parent aligned `deployment/observability.md` with the purger's metric (`account_deletion_seconds`), reviewed the migration, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3927_vault_membership_wiring | Done | Membership = `nodes.vault_id` (migration `20261114100000_add_tree_node_vault_membership`: column, composite FK to `vaults (id, account_id)` `ON DELETE SET NULL (vault_id)`, partial index; up/down/up checked). 20 creates the vault and claims the root through the new 02 port `VaultRoots::claim_root` in one transaction (closes the batch 58 race); 02 copies `vault_id` into new nodes and conflict copies; publish flags vault entries (no media job); moves across a vault boundary are `vault_boundary` (409); 03 refuses uploads into a vault above `max_vault_file_bytes` at start and again in the commit effects (`vault_file_too_large`, 413); 06 refuses shares of vault files. AC-1..AC-6 PASS plus two race tests (30 rounds) |
| AZ-3820_deletion_purger | Done | `DeletionPurger` (`Janitor`): per step its own transaction (`lock_deleting` `FOR NO KEY UPDATE SKIP LOCKED`, one `erase_batch`, commit); hooks walked in order in passes, finalize (`finalize_deletion` + `account_deleted` audit) only after a pass where every hook reports 0 on its first call (03 queues blobs that 02 counts after 02 reported 0); budget `account_delete_cycle_budget`; ERROR per failed step, WARN past `account_delete_alert_after`; `account_deletion_seconds`. IT-07-9..13 with the real 02, 03, 05, 06, 07a hooks, incl. a no-self-deadlock test with a control that does block under `FOR UPDATE` |

## Contract and shared changes

- `sc_error`: `vault_file_too_large` 413, `vault_boundary` 409 (the second is not an owner-named code: the E-30/c01 move rule needs it).
- `contracts`: `UploadError` / `CommitServiceError::VaultFileTooLarge`, `TreeServiceError::VaultBoundary`, `NodeView.vault`, `LiveFile.vault`, `VaultRoots` (part 2 1.3.0, part 3 1.5.0); `AccountAdmin::lock_deleting`, `AccountInfo.deletion_requested_at` (part 1 1.6.0).
- `sc_config`: `account_delete_cycle_budget` 5 s.
- 11: mappings and route error lists for the new codes. `sync-core` `wire_table.rs`: the two codes added to its unexpected arm to compile (client handling to be decided by 13's owner).

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3655 passed, 2 failed, 28 ignored; `check-no-sql-now` failed. All three fixed by the parent and re-run green:
- `check-no-sql-now`: two new test inserts (01 `account_lock_deleting.rs`, 07 `tests/purger/drain.rs`) used SQL `now()`; replaced by fixed timestamps; the gate script re-run OK and both tests pass.
- 05 `migration::up_down_up_adds_and_removes_exactly_the_feed_objects`: it reverted every migration from the feed's on and compared `nodes` with the full schema, so the new vault migration (`nodes.vault_id`) broke it. It now reverts the later migrations first, measures the feed migration's own objects in isolation, and compares the re-applied schema with the full one; 7 of 7 feed migration tests pass. The other components' migration tests (01, 02, 03, 04, 06, 07a, 08, 20) passed in the gate.
- 04 `window::a_consumer_slower_than_the_disk_stays_at_one_chunk_ahead` (batch 64): timing-dependent under load (the disk read could outlast the fixed pause, so the consumer found nothing ready and the window widened). It now waits until the read has completed (`chunk_reads`) before the pause; same assertions; 3 of 3 runs green.
`rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); the batch adds migration `20261114100000` (up/down/up covered by 02's migration test).

Owner decisions needed:
- A file deleted before its folder became a vault can be revived by an edit inside the vault with no membership (then thumbnailed and shareable): refuse vault creation over tombstoned children, or set membership on revival?
- Operator restore can place a vault file in the plain `Restored` folder (it keeps `vault_id`): accept, or refuse restoring vault files there?
- `vault_boundary` (409) was added without an owner decision.

Warnings (recorded):
- Clients (sync-core) treat `vault_file_too_large` and `vault_boundary` as unexpected errors.
- The purger keeps calling 02 until 04's GC removes queued blobs (`gc_interval` 5 min): relevant for scheduling (E-22).
- The camera-backup rule for vaults (spec flag 3) stays open.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | `nodes.vault_id` did not exist | AZ-3927 | Migration added (02 column, 20 key) | clear |
| 2 | Batch 58 race (child created during vault creation) | AZ-3927 | Closed by `claim_root` under the parent locks | clear |
| 3 | Revived deleted file inside a vault | AZ-3927 | Owner decision above | unclear |
| 4 | Restore of a vault file into `Restored` | AZ-3927 | Owner decision above | unclear |
| 5 | Finalize hit an FK on `blobs` without the extra pass | AZ-3820 | Finalize after an all-zero pass | clear |
| 6 | Metric name mismatch (`account_deletion_seconds` vs the deployment doc) | AZ-3820 | Doc aligned by the parent | clear |
| 7 | 07 description hook order (20 before 02) | AZ-3820 | Fixed to the 2026-11-05 decision | clear |

## Commit

`[AZ-3927] [AZ-3820] Vault membership, deletion purger` — Loop: 1

## Next Batch

AZ-3703 (06 security suites, owner option A) and AZ-3760 (03 integration pass), then recompute.
