# Cumulative Review — batches 61 to 63 (loop 1)

**Date**: 2026-11-14
**Range**: `d699fe5..6d0c196` (batch 61 `e28d2b7`, batch 62 `b526f74`, batch 63 `6d0c196`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review; findings were fixed or recorded. This pass looks across the three batches. 135 files changed, about 10,600 lines added: 03 transfer (commit_upload, commit_batch with receipts, downloads; `CommitService` and `DownloadService` impls), 06 sharing (visitor open, authorize_content, account erase), 02 tree (operator restore, security and acceptance suites), 07 admin (account view, deletion request), 01 (account profile), and a 04 fix for concurrent `.epoch` writes.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds (`crate_graph` green in every gate). New normal edges: 06 on `tokio` (`rt`, workspace dep) for `spawn_blocking`; 03 on `serde_json` (workspace dep, receipts). Component edges are dev-only (03 on 05; 06 on 04, 05, 07a, testkit; 07 on 01, 02, 04, 05, 07a) |
| Shared contracts changed | Three, parent-approved: `EditedSince` removed from `CommitServiceError`/`EntryError` (61, part 2 1.2.0, 11 and 03 updated); `AccountAdmin::profile` + `AccountProfile`, `IdentityView` moved and re-exported (62, part 1 1.5.0); `AccountDeletionService::request_as_operator` (62) |
| Migrations | None (`commit_receipts` already existed) |
| Lockfile | No new external package; workspace-crate edges only |
| System-scope allowlist | One reviewed line: 06 `lookup_repo.rs` (visitor token lookup before any account is known) |
| Production code quality | No `unwrap()` and no `#[allow]` added in production sources |
| Owner decisions applied | Option A for AZ-3738, 3698, 3715, 3719, 3754, 3755, 3700; AZ-3819 read method; gate S12 Safari on the owner Mac |
| File length | No file over 500 lines; closest: `03 tests/transfer/start_support.rs` (467) |
| Trait impls | `CommitService` and `DownloadService` now implemented by 03; `PublicShareService` still inherent methods only (waits for `verify_password`, `authorize_derived`) |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | `FileEpochMarker::write` raced on its fixed temp name (two first sign-ins initialising the feed at once failed one with a 500); found by the batch 63 gate | Fixed in batch 63 (serialised writes, regression test). `KeyCheck`'s volume file uses the same `replace_file`, but is written only by the operator's `keycheck-init` | Fixed (High) |
| 2 | Owner decisions open: AZ-3755 suffix ranges (`bytes=-N` served Full; needs a `ByteRange` change); AZ-3700 AC-6 (best-effort create/revoke audit vs `sc_audit` fail closed); AZ-3698 pending-cascade timing difference; AZ-3738 foreign base revision ignored | Owner | Medium |
| 3 | Race warnings collected for the race-suite task E-14/c11: two concurrent `commit_upload` of one upload (500), abort vs commit deadlock, lazy load vs abort (59) | E-14/c11 | Low |
| 4 | `EntryError` has no `NotFound`; `DeletionError` has no `NotFound` for the operator path | Contracts owner, when E-21/E-22 map them | Low |
| 5 | Services still unwired in production: `PgTreeRestore`, `PgShareLifecycle`, 07 account and deletion services, 03 commit/download | AZ-3789 composition root, E-22 CLI | Low (expected order) |

## Open findings carried forward

| # | Finding | Status |
|---|---|---|
| 1 | OpenAPI parameters without a pattern | Open, route registry follow-up |
| 2 | `contracts::MasterKey` does not zeroize on drop | Open, contracts owner |
| 3 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 4 | `max_vault_header_bytes` default outside `sc_config` | Open, Low |
| 5 | Timing-flaky under load: 07a lock wait, sync-core `hash_guards`, server `config_binary` listener probe | Open, Low |
| 6 | Generated Kotlin and Swift bindings print token fields | Open, E-27/E-28 |
| 7 | Android JNI code and UniFFI CI legs never run | Leftover `2026-11-13_az-3968-android-legs.md` |
| 8 | `rust-lint.sh` steps 3-4 cannot run here (no `psql`) | Environment |
| 9 | Test log capture copied three times | Shared helper when next needed |
| 10 | Safari S12 run on the owner Mac | Leftover `2026-11-14_az-3931-safari-mac-run.md` |
