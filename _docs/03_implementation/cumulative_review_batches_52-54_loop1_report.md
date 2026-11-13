# Cumulative Review — batches 52 to 54 (loop 1)

**Date**: 2026-11-13
**Range**: `9894dac..2c87798` (batch 52 `db802ed`, batch 53 `536b08b`, UTC rule `363ed9d`, batch 54 `2c87798`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review with every finding fixed, decided by the owner or recorded. This pass looks across the three batches for drift that no single batch review would see. 140 files changed, about 12,200 lines added; most in 02 tree (janitors, conflict rules, reader), 01 identity (account creation, limits, audit, invites), and sync-core (large upload, UniFFI surface).

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds. New dev edges only (identity on tree, blobstore, feed, quota); new normal edges are helpers only (identity on `sc_audit`, `sc_ratelimit`; tree on `sc_observability`). Identity's production code imports no hook implementer |
| Shared contracts changed | Three, each approved by the parent and documented: `SignInService` gains `client_ip` (batch 53), `InviteError::NotFound` and `OperatorIdentityError::{NotFound, NotAnAdmin}` (batch 54), `ConnectError::TrustStoreUninitialized` (batch 54). All additive except `client_ip`, whose callers (11 handlers, test doubles) were updated in the same batch |
| Lockfile growth | No new external package (path and feature entries only); UniFFI stays at one `=0.32.2`; `jni` 0.22.4 was already present through the verifier |
| System-scope allowlist | One entry added (`02_tree/src/maintenance_repo.rs`, cross-account janitors, ADR-014). Identity's system-scope work stays in `auth_repo.rs` |
| One canonical path | One redemption path for invites, setup token and re-bind (extended, not duplicated); one retry wrapper (`run_retrying_each`) for publish, erase, maintenance; one abuse counter for device grants and sign-in. Tree ports are served by four structs (`PgTreeService`, `PgTreeLifecycle`, `PgTreeMaintenance`, `PgTreeReader`), each with only the ports it needs, so no constructor of another crate's tests had to change |
| File length | No source or test file over 500 lines; `publish.rs` (489), `client.rs` (494), `auth_repo.rs` (478) are close to the cap |
| Feature gates | `uniffi` off by default, checked by `uniffi_isolation.rs` (native and wasm); the Android JNI code is `cfg(target_os = "android")` under the feature |
| Owner decisions | 14 decisions recorded in specs and component docs, two of them corrections of questions the parent framed on incomplete information (listing page cap, Android init mechanism) |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | Test support still duplicated (statement capture in 5 suites, boundary `AccountGuard` double in 3); batches 52-54 added more users of both | AZ-4250 (open) — schedule it in a batch with no other worker in 01, 02, 05, 08 tests | Medium (maintainability) |
| 2 | A tombstoned vault root would make the tombstone purge fail on every run | Recorded on AZ-3923 (High) before the first vault can be created | High (deferred, no vault exists yet) |
| 3 | Generated Kotlin and Swift bindings print token fields | Recorded in `uniffi_surface.md` for E-27/E-28 | Medium (security, mobile hosts) |
| 4 | The Android JNI code and the UniFFI CI legs have never run | Leftover `2026-11-13_az-3968-android-legs.md`; CI wiring of `bindings_check.sh` and `--features uniffi` in `.github/workflows` | Medium |
| 5 | Several shared-interface gaps were found only during implementation (caller address on sign-in, refusal variants, trust-store error) | Process: worker briefs now ask for contract gaps first; the parent approves additive changes in the same batch | Low (process) |
| 6 | `sync_conflict_copies_total` and the janitor row count have no metric/log field; `NodeView.thumb_state` is missing | E-09 / E-13 owner; AZ-3718 | Low |

## Open findings carried forward

| # | Finding | Status |
|---|---|---|
| 1 | OpenAPI parameters without a pattern | Open, route registry follow-up |
| 2 | `contracts::MasterKey` does not zeroize on drop | Open, contracts owner |
| 3 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 4 | `max_vault_header_bytes` default outside `sc_config` | Open, Low |
| 5 | 07a lock-wait test timing-flaky under load | Open, Low |
