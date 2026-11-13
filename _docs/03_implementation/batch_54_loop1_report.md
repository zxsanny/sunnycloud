# Batch Report

**Batch**: 54
**Tasks**: AZ-3707_tree_reader (done), AZ-3712_tree_blob_ownership_check (done), AZ-3688_setup_token_invites_operator_ops (done), AZ-3968_sync_uniffi_surface (done; Kotlin, Android compile and device legs not run)
**Date**: 2026-11-13

Three parallel workers on disjoint crates (`02_tree`, `01_identity`, `sync-core` + `sync-core-proto`). Before the start the parent added the optional `uniffi` feature (UniFFI `=0.32.2`, as vault-core) to sync-core and sync-core-proto; mid-batch it added the Android-only `jni` 0.22.4 dependency and made `listing_max_limit` default to 1000 (sc_config, tree config, reader). Lockfile: path and feature entries only, no new package. The parent approved two contract changes (invite and operator refusal variants; `ConnectError::TrustStoreUninitialized`), wrote every document change, reviewed, ran the gate and committed.

## Owner decisions 2026-11-13

1. Batch 54 as specified, with the proposed defaults: revoking an unknown, revoked or redeemed invite answers `NotFound` alike; `reset_admin` also revokes device tokens; a foreign or unknown blob is `BlocklistMismatch`; the Android trust-store setup mechanism (first answer: a UniFFI call, corrected below).
2. `listing_max_limit` defaults to 1000. Correction: the parent first framed this as "no extra default", which left the page size uncapped (a request could ask for any number of rows); the owner then chose 1000.
3. Android trust-store initialisation is an exported native JNI function the app calls with its `Context` (a UniFFI call cannot pass the `Context`; the earlier choice was made on an incomplete summary).
4. Conflict-copy names are UTC only (between batches 53 and 54, commit `363ed9d`).

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3707_tree_reader | Done | `PgTreeReader::new(db, &TreeConfig)`: `get_node`, keyset `list_children` (`limit + 1` for the next page, cursor = raw `name_key`, garbage cursor `InvalidCursor`, page cut to `listing_max_limit`), `find_live_node`, `resolve_live_file` with the parent-chain walk, `root_node`; one short account-scoped transaction per read; foreign ids answer like random ids. AC-1..AC-8 on PostgreSQL 18 with DS-02 (10,000 entries built through `create_folder` + `publish`); AC-3 checks the plan under the API role with row-level security on (`nodes_parent_name_key` + `revisions_pkey` nested loop, no seq scan, sort or bitmap). PT-02-1 `NOT-RUN(HW-REF)`; information only (debug, workstation): full 10,000 listing p95 148 ms (target 200), one 1,000 page p95 17.8 ms (target 40). The `TreeReader` and `TreeService` trait impls wait for AZ-3716's revision reads |
| AZ-3712_tree_blob_ownership_check | Done | Ownership is checked by the composite foreign key `revisions (blob_id, account_id) → blobs (id, account_id)` inside the entry's savepoint, not through `BlobCatalog::get_ref` (it answers `None` for an `uploading` blob, the state every large upload commits, and takes no transaction). AC-1..AC-4: foreign blob refused with nothing written; foreign and random id give the same `BlocklistMismatch`; one bad entry in a batch fails alone under one seq; an own uploading blob publishes |
| AZ-3688_setup_token_invites_operator_ops | Done | `IdentityInvites` implements `InviteService` and `OperatorIdentityOps`. One redemption path, extended: the setup token creates the first admin only while no account exists; a re-bind invite binds a new identity to the target admin with no hook. `reset_admin` in one transaction revokes sessions and device tokens, denies uncollected grants, deletes login codes, removes identities at once, supersedes earlier re-bind invites, with mandatory `admin_reset`, `session_revoked` and `identity_rebound` events. AC-1..AC-6 plus an 8-racer setup race; typed refusals `InviteError::NotFound` (11: 404) and `OperatorIdentityError::{NotFound, NotAnAdmin}` |
| AZ-3968_sync_uniffi_surface | Done, legs not run | Feature `uniffi`: derives and converters on 92 exported names, flat `SyncError`/`ConnectError`/`BatchRunError`, foreign twin host traits with `Foreign<T>`, `HttpSyncClient` object with `cancel()` and `limits()`; Android `TlsTrustBridge.nativeInit` JNI export and `ConnectError::TrustStoreUninitialized` before init. AC-1 (feature off: no UniFFI crate, native and wasm), AC-2 (Kotlin and Swift bindings, `bindings_check.sh`), AC-4 (320 tests without, 326 with the feature, differing only by the 6 feature-only tests), AC-5 (one uniffi 0.32.2) pass. Not run: AC-3's Kotlin leg (needs `uniffi_surface.md` open point 2), the Android compile (no Android target here, not installed) and AC-6's device run (`NOT-RUN(EMU)`); leftover `2026-11-13_az-3968-android-legs.md` |

## Contract and document changes (parent)

- `contracts::InviteError::NotFound`, `OperatorIdentityError::{NotFound, NotAnAdmin}` (code; `part1_registries_identity_admin_quota.md` 1.3.0); 11 maps `InviteError::NotFound` to 404.
- `operator_identity_ops.md` 1.2.0 (frozen), `account_creation_hooks.md` 1.4.0 (setup and re-bind redemption).
- `tree_reader.md` 1.1.0 (provider, `limit = 0`, cursor, tombstoned ancestor, `listing_max_limit` 1000); 02 description and `blob-registry-gc.md`: ownership by the composite foreign key.
- `sync_api.md` 1.3.0 (`ConnectError::TrustStoreUninitialized`, `InvalidConfig{field}` listed); `uniffi_surface.md` 1.2.0 (surface, foreign twins, JNI init, generated bindings print secrets).
- `01_helper_config.md`, 02 description: `listing_max_limit` 1000.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (3168 passed, 0 failed, 21 ignored), wasm build and the four check scripts all exit 0; the `--features uniffi` clippy and test legs were run by the worker (326 passed).

Fixed during the batch: the uncapped listing page size (default 1000 now; `page_size` always clamps, test with `u32::MAX`). Warnings (recorded):
- Generated Kotlin data classes and Swift structs print `Credential`, `SessionTokens` and `DeviceToken` field values in `toString`/`description` (the Rust redaction does not cross the boundary): the mobile hosts (E-27, E-28) must never log these objects. Recorded in `uniffi_surface.md`.
- The Android JNI code has never been compiled; the `#[no_mangle]` symbol must be checked in the built `mobile_backup` `.so` (`nm -D`).
- `bindings_check.sh` borrows vault-core's bindgen binary and neither it nor the `--features uniffi` test leg is in CI (`.github/workflows`).
- `NodeView` has no `thumb_state` yet (contracts change with AZ-3718).
- `InviteInfo` has no role field; a re-bind of a disabled admin consumes the invite, then sign-in answers `AccountNotActive`; on an empty install a setup redemption racing an operator admin-invite redemption can make two admins.

## For 11 and 12

- 12: `IdentityInvites::new(db, clock, &IdentityConfig, AuditWriter)` as `Arc<dyn InviteService>` (11) and `Arc<dyn OperatorIdentityOps>` (CLI); `ensure_setup_token()` at `serve` start (print `Some` once; `None`: nothing, `setup-token` exit 2); `admin invite|create` → `create_admin_invite()`; `admin reset <id>` → `reset_admin(id)` with exit codes for `NotFound` and `NotAnAdmin`.
- mobile_backup (E-27): `sync-core` with `features = ["uniffi"]`, `uniffi = "=0.32.2"`, `crate-type = ["cdylib", "staticlib", "lib"]`, its own `setup_scaffolding!` and bindgen bin; foreign host objects through `sync_core::Foreign`; Kotlin `TlsTrustBridge.nativeInit(applicationContext)` once per process.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | `TreeReader`/`TreeService` traits need AZ-3716's revision reads; the reads are inherent on `PgTreeReader` until then | AZ-3707 | As implemented | clear |
| 2 | DS-02 is built per test through `create_folder` + `publish`; there is no `tmpl_listing` template database (about 15 s per test) | AZ-3707 | As implemented | clear |
| 3 | `root_node` answers `NotFound` only for an erased account; a live file with no current revision is `Unexpected` | AZ-3707 | As implemented | clear |
| 4 | The reset removes the admin's identities at reset time, not at redemption | AZ-3688 | Accepted by the parent (01: nobody signs in as that admin until the invite is redeemed) | clear |
| 5 | Revoke deletes the open invite row (no `revoked_at` column); `list` shows all invites to every admin | AZ-3688 | As implemented | unclear |
| 6 | The JNI package `org.sunnycloud.backup` is the producer's choice | AZ-3968 | E-27 may change it with the symbol | unclear |
| 7 | UniFFI's macros bring `syn` 2 while the workspace uses `syn` 3: one copy per major | AZ-3968 AC-5 | Accepted | clear |

## Commit

`[AZ-3707] [AZ-3712] [AZ-3688] [AZ-3968] Tree reader, invites, UniFFI surface` — Loop: 1

## Next Batch

Recompute the ready tasks.
