# Batch Report

**Batch**: 52
**Tasks**: AZ-3714_tree_cascade_purge_janitors (done), AZ-3685_invite_redemption_account_creation (done), AZ-3949_sync_nothing_to_send_spike (done), AZ-3953_sync_resend_bound_spike (done)
**Date**: 2026-11-13

Three parallel workers on disjoint crates (`02_tree`, `01_identity`; the two sync spikes as one read-only worker). Before they started the parent added identity's dev-dependencies on the hook crates (02, 04, 05, 07a); during the batch it added `sc_observability` to the tree crate and the tree's maintenance repository to the run-system allowlist. Only the parent touched `Cargo.lock` (path entries only). The parent wrote every document change, reviewed, ran the gate and committed.

## Owner decisions 2026-11-13

1. AZ-3714 and AZ-3685 built as specified (complexity budget).
2. With `email_allowlist` set, a verified email not on the list refuses the invite redemption (`InvitationRequired`, nothing created).
3. A new account's display name is the provider's name claim, else its username (GitHub `login`, OIDC `preferred_username`), else `User`.
4. Lost-commit recovery, option (a): 03 keeps a committed session's `(account, client_key)` → revision until `upload_session_ttl` and `start_upload` answers `NothingToSend{revision}` for it, whatever the name holds now; one server file in every branch. Written into `upload_service.md` 1.2.0, `commit_service.md` 1.2.0 and as AC-6b of AZ-3734 and AZ-3738 (both still to do).

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3714_tree_cascade_purge_janitors | Done | `PgTreeMaintenance` (`cascade_batch`, `purge_tombstones`) and `TreeJanitor::{cascade, purge}` (`Janitor`). Cascade claims pending folders with `SKIP LOCKED`, marks live children up to the limit without seq values, subfolders get `cascade_pending`, the flag clears with the subtree. Purge claims leaf tombstones past `tombstone_horizon`, queues each revision's blob first (`Deleted`, `AfterRetention`), deletes revisions then nodes, and raises the purge horizon last, once per account. System scope (allowlisted `maintenance_repo.rs`), deadlock retried once. AC-1..AC-4 pass on PostgreSQL 18 with the real 04 queue and 05 horizon and reader (a purge killed by `pg_terminate_backend` after 2 batches is finished by the next run, once per node; the horizon update is the last statement of all 10 purge transactions). WARN for a cascade batch slower than the interval, INFO per batch with its duration. AC-5 (PT-02-4) `NOT-RUN(HW-REF)`; information only: one 5,000-node batch 182.7 ms (debug, local) |
| AZ-3685_invite_redemption_account_creation | Done | The registration branch of `complete`: one system-scope transaction reads the open invite, inserts the account (role from the invite, name per decision 3) and identity, runs every registered hook's `initialize`, then redeems with the single conditional update. AC-1..AC-4 and AC-75 pass with the real hooks of 02, 05 and 07a: 200 iterations of 2 and of 16 racers, 0 iterations with two accounts; a failing hook leaves nothing and the invite redeemable. Email rule per 01 §5 and decision 2 (no allowlist: no email read, GitHub `/user/emails` not called). The hook list is set once and fails closed when unset. `account_creation_hooks.md` 1.2.0 frozen |
| AZ-3949_sync_nothing_to_send_spike | Done | Client rule recorded (`sync_api.md` 1.1.0 § Lost-commit recovery): after a lost commit answer the client repeats `start_upload` once with the identical request; per answer: `NothingToSend` ends it, the same session resumes, a typed error is returned. IT-13-16 (B) unblocked; (C) resolved by decision 4. Downstream specs updated: AZ-3962, AZ-3971, AZ-3656 |
| AZ-3953_sync_resend_bound_spike | Done | Client lanes `min(4, max_lanes, lane_limit)` = 4, no extra cap; worst-case re-send 4 x 2 MiB + 8 MiB = 16 MiB (AC-3). 10 → 16 MiB in the 13 tests and description, E-23, FT-P-01, NFT-PERF-05, NFT-RES-01, AZ-3962, AZ-3972; G-03-1 row extended; AZ-3957's concern closed. No code change: the lane cap is already 4 |

## Code Review Verdict: PASS_WITH_WARNINGS

Fixed during the batch:
- `lifecycle_repo::DELETE_LEAVES` (AZ-3717) used `DELETE … WHERE id IN (SELECT … LIMIT n)`, the shape that marked more than the limit in the cascade (44/35/21 instead of 20); both picks are now `MATERIALIZED` CTEs, and the erase test asserts one call never removes more than its limit.
- The janitor's two log points needed `sc_observability` (added) and the maintenance repository needed the run-system allowlist entry (added; `check-db-gates` 3/3).

Warnings (recorded):
- A tombstoned vault root would make the purge fail on `vaults_root_node_id_account_id_fkey` on every run. No vault can be created yet (the vault service has only its erase hook); recorded as a High concern on AZ-3923 (vault create) so 02 refuses deleting a vault root or the purge skips it before the first vault exists.
- 05's `initialize` writes the installation's `.epoch` marker file inside identity's transaction when it creates the first feed; recorded as a known exception in `account_creation_hooks.md`, open for the E-12 owner.
- The INFO line of a janitor batch carries its duration, not its row count: the `sc_observability` field schema has no count field.

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (3068 passed, 0 failed, 20 ignored), wasm build and the four check scripts all exit 0. The host restarted at 07:05 during the batch (scratchpad cleared, test container stopped); `git fsck` was clean and no working file was empty.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | With the allowlist set, an identity with no usable email (absent, unverified, relay) and a valid invite is refused | AZ-3685, decision 2 | As implemented (fail closed) | unclear |
| 2 | "Relay address" is the domain `privaterelay.appleid.com` only | AZ-3685 AC-3 | As implemented; other relay domains not filtered | unclear |
| 3 | Setup token and re-bind invites are refused by the creation path until E-15/c08 | AZ-3685 | As implemented | clear |
| 4 | The same provider identity in two tabs is decided by the unique key of `identities`, mapped to `InvitationRequired` | AZ-3685 | As implemented | clear |
| 5 | The DB role values are `user` and `admin`; the spec's "member" means `user` | AZ-3685 | As implemented | clear |
| 6 | Purged revisions' blobs are queued `Deleted` with `Due::AfterRetention`; an earlier due time is kept | AZ-3714 | As implemented | unclear |
| 7 | Cascade-marked descendants get the cascade's time as `deleted_at`, not the folder's tombstone time | AZ-3714 | As implemented; purge is leaf first, so correct | unclear |
| 8 | The retention purge of `derived` rows with no revision left (data model) is not part of AZ-3714 | data_model | Not done; belongs to 08's retention | unclear |
| 9 | The `upload` error list still names `NameConflict` and `EditedSince`, which commit never answers | AZ-3949 | `sync_api.md` open point 8, for AZ-3962 | clear |
| 10 | The `sunnycloud-pg18` container had stopped during the batch; the identity worker restarted it (same container) | — | Environment only | clear |

## Commit

`[AZ-3714] [AZ-3685] [AZ-3949] [AZ-3953] Tree janitors, invite account creation, sync spikes` — Loop: 1

## Next Batch

Recompute the ready tasks (AZ-3710 publish conflict outcomes is now unblocked by AZ-3714).
