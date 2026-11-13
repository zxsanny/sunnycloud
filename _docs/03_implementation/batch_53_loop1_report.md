# Batch Report

**Batch**: 53
**Tasks**: AZ-3710_tree_conflict_rules (done), AZ-3962_large_upload_executor (done), AZ-3691_identity_abuse_limits_audit (done), AZ-3694_provider_linking_and_list (done)
**Date**: 2026-11-13

Three parallel workers on disjoint crates (`02_tree`, `sync-core`, `01_identity`). The parent added identity's `sc_audit` and `sc_ratelimit` dependencies before the start (lockfile path entries only), approved one contract change mid-batch (the caller's address on `SignInService`) and widened the identity worker's ownership to the contracts file and 11's sign-in handlers and test doubles for it. The parent wrote every document change, reviewed, ran the gate and committed.

## Owner decisions 2026-11-13

1. AZ-3710 and AZ-3962 built as specified (complexity budget).
2. Reviving an ancestor before the cascade marked its descendants brings the unmarked ones back live; marked ones stay deleted (`data_model.md` §5.1).
3. Invalid tokens presented at sign-in: 100 per minute per IP (AC-20's number).
4. Mandatory (fail-closed) identity audit events: account created, role granted, provider linked or unlinked, sessions revoked, admin reset; sign-in records best-effort.
5. Every sign-in completion that ends without a session, and every wrong device-approval code, counts as a failed step.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3710_tree_conflict_rules | Done | `ConflictCopied` (the superseded revision row moves with its blob to a new sibling with the conflict-copy name; both nodes stamped), `CreatedAsCopy` (taken name on a new file; `INSERT … ON CONFLICT DO NOTHING` keeps one savepoint per entry), `Revived` (file and every tombstoned ancestor revived, `cascade_pending` cleared, revived nodes stamped; a taken ancestor name gets a conflict-copy name). Names only from `sc_names::conflict_copy_name` with a tree lookup; a lost name race retries with the next free name, at most 8 times. `OutcomePending` deleted. Every AC on PostgreSQL 18 with the real 05 stamp and reader, the owner's revive-before-cascade case and the 255-byte truncation included; 05's `via_tree` and 08's effect tests still pass |
| AZ-3962_large_upload_executor | Done | `HttpSyncClient::upload`: lanes `effective_lanes(max_lanes, lane_limit)`, `X-Blake3` per block, done only on 204; block answers mapped per the spec (5xx/429/reset transient; 507/401/other refusals end it); resume re-reads `need_blocks`; key reused for the same hash; checkpoint `Uploading` before the first block, `Committing` before commit, cleared only after an outcome; source identity re-check before commit, re-hash and restart after `BlocklistMismatch`; the lost-commit rule of `sync_api.md`; progress for 204 blocks only; jitter injectable, durations from config. AC-1..AC-9 against the scripted fixture (the resume test 15 runs in a row). IT-13-16 (C) not modelled (waits on AZ-3734/AZ-3738) |
| AZ-3691_identity_abuse_limits_audit | Done | Shared `AbuseCounter`: wrong device-approval codes trip at 5/session, 20/account, 50/IP (AC-1, AC-2); failed sign-in steps at 10/min/IP and invalid tokens at 100/min/IP (AC-3) through the new `client_ip`; one `abuse_trip` event (limit and address in the `ip` column) and one WARN (limit only) per trip (AC-4); events ids only (AC-5); a failing audit write keeps approval and sign-in with an ERROR and fails a session revoke closed (AC-6 reworded to the owner's decision); `account_created` is the last statement of the account-creation transaction and rolls it back on failure; `provider_linked` in the link transaction |
| AZ-3694_provider_linking_and_list | Done | `begin_link`, `complete_link`, `list_providers`; `IdentitySignIn` implements `contracts::SignInService`. AC-1..AC-5: a second provider links and both sign in to one account; another account's identity is `IdentityTaken`; no email used or stored; replayed, foreign, cross-account, sign-in-state and expired link callbacks are `UntrustedState` (the link state hash binds the browser and the account, no migration); the provider list and `setup_required` (true while no account exists) |

## Contract and document changes (parent)

- `contracts::SignInService` (code, approved by the parent): `client_ip: IpAddr` on `BeginSignIn`, `complete`, `complete_link`, `exchange_mobile_code`; 11 passes `caller.client_ip` as for device grants. Doc: `part1_registries_identity_admin_quota.md` 1.2.0.
- `tree_commit.md` 1.1.0 and `data_model.md` §5.1: `Revived` walks every tombstoned ancestor to the root; a taken own name answers `CreatedAsCopy` (aligned with 03's `commit_service.md`).
- `sync_api.md` 1.2.0: `UploadRequest.local_key` and the key-reuse rule; `upload` errors without `NameConflict`/`EditedSince` (open point 8 closed), with `NotAFolder` and `Io`; open point 9 (below). Same rows in the 13 description and E-23.
- `account_creation_hooks.md` 1.3.0: step 5, the mandatory `account_created` event.
- AZ-3691 AC-6 reworded to the owner's audit decision.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (3124 passed, 0 failed, 20 ignored), wasm build and the four check scripts all exit 0.

No defect found in the reviewed paths (publish outcomes, the upload loop's checkpoint handling, the limiter and audit placement). Warnings (recorded):
- `conflict_copy_time_zone` is accepted by the config (for example `Europe/Kyiv`) but not applied: helper 06 formats UTC only and `PgTreeService::new` has no `TreeConfig`. Names are UTC, the default. Owner question for the next batch: validate the key as UTC-only, or apply zones (time-zone data in helper 06 and the config handed to the service).
- `sync_conflict_copies_total` is not emitted: wiring a counter needs a `PgTreeService` constructor change and the composition root; ownership is open (E-09 / E-13).
- `upload` and `upload_small` are inherent methods, not `SyncApi` trait methods (moving them needs every `SyncApi` test double to implement them); `sync_api.md` open point 9.
- `HashedFile` carries no source identity: the before-commit check compares with the identity read when `upload` starts, plus the per-block hash check (open point 9).
- A checkpoint-store failure is logged and the upload continues; failing hard needs a `SyncError::Store` (open point 9).
- `CommitOutcome::ConflictCopy` carries no revision id (open point 9).
- `Unexpected` (server fault) does not count as a failed sign-in step, so a server fault cannot lock users out.

## For the composition root (12)

`IdentitySignIn::new(db, clock, &config, audit, limiter)`, `IdentitySessions::new(db, clock, &config, audit)`, `IdentityDeviceGrants::new(db, clock, geo, &config, limiter, audit)`; one `Arc<AbuseLimiter>` shared by sign-in and device grants, built with `LimitTable::new(device_grant_ip_spec, LimitSpec::new(100, Duration::from_secs(60)))` (`InvalidTokenIp` 100 per minute, owner). Routes 11 still needs: `GET /api/v1/auth/providers` → `list_providers`, `POST /api/v1/identity-link-attempts` → `begin_link`; the shared provider callback must choose `complete_link` or `complete` (for example by a link-binding cookie set by the link-attempt route).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | A live folder can sit below a tombstoned one until the cascade marks it, so `Revived` walks to the root (spec: "up to the first live one"); an edit of a live file under a tombstoned ancestor also revives | AZ-3710 | As implemented; contract updated | clear |
| 2 | `AuditEventKind` has no kind for account created or provider linked; they use `sign_in` with detail `action` | AZ-3691 | A dedicated kind needs a migration (CHECK) | unclear |
| 3 | Refresh-token reuse revocation stays best-effort: an audit failure never undoes a security revocation | AZ-3691 AC-6 | As implemented, commented | clear |
| 4 | Sign-in failure audit records are not written (flood risk); a mobile sign-in is audited at the callback | AZ-3691 | As implemented | unclear |
| 5 | Proposed defaults: re-linking an identity already on the account is an idempotent success; `ProviderInfo{key, display_name}` with fixed display names per kind | AZ-3694 | Owner may override | unclear |
| 6 | `begin_link` takes no `return_to`; identity stores "/" and checks the host at `complete_link` | AZ-3694 | As implemented | unclear |
| 7 | `UploadRequest` needs `local_key` (the checkpoint key) | AZ-3962 | Contract updated | clear |
| 8 | `GET /uploads/{id}` returns no `lane_limit`; the commit answer's extra `revision_id` on a conflict copy is ignored | AZ-3962 | As implemented | clear |
| 9 | The invite-token limit path at `complete` is not tested (needs the hook rig) | AZ-3691 | Follow-up test | clear |

## Commit

`[AZ-3710] [AZ-3962] [AZ-3691] [AZ-3694] Conflict rules, large upload, sign-in limits` — Loop: 1

## Next Batch

Recompute the ready tasks; owner question on `conflict_copy_time_zone` first.
