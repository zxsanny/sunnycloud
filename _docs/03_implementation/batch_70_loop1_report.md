# Batch Report

**Batch**: 70
**Tasks**: AZ-3789_composition_root (partly: stays in `todo/`)
**Date**: 2026-11-14

Owner decisions 2026-11-14: plain lazy builders (no registry), vault crates behind a `vault` feature of the server crate (off by default), 08's media effect takes the account from each commit. One worker on the server crate. The parent also wrote the cumulative review of batches 67-69, created three follow-up tasks for the blockers (AZ-4309, AZ-4310, AZ-4311; Jira + specs + dependency table), ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3789_composition_root | Partly | Built: one lazy builder per component on `ServiceContext` (built once, on first use; `ServiceContext::built()` lists them); base clock, API `Db`, `AuditWriter`, `Metrics`, one `AbuseLimiter` (01, 06), shutdown tokens; `ChangeFeed` gets its shutdown at construction. Late lists in order: account hooks 06, [20 with `vault`], 02, 03, 08, 05, 07a (to 01's sign-in and 07's `DeletionPurger`); blob-owner cleanups 02, 08 (to 04's `GcSweeper`); 11 janitors on their owners' keys in one scheduler; health contributors (blobstore, media); doctor checks (master_key, blob_volume, blob_retention, blob_files_missing, media_worker); readiness `database` (new adapter over `Db::health`) then `scheduler_alive`. `sunnycloud-vault-service` optional behind `vault`. Tests: AC-2..AC-6 (in-process `/readyz` 200 with both checks; every registered janitor cycles against the real database; default build links no vault crate) |

## Not done (blocked, nothing stubbed)

| Blocker | Follow-up |
|---|---|
| `contracts::AdminService` has no implementation (07 inherent methods; `AdminError` lacks `Forbidden`; `health` signature differs) | AZ-4309 (CC-3) |
| `contracts::PublicShareService` has no implementation (`authorize_derived` missing in 06) | AZ-4310 |
| `CommitEffect::on_entry` cannot see the account, so the shared media effect is not registered and 03's injected effect list is empty | AZ-4311 (`PublishedEntry.account_id`) |
| 01's `IdentityMaintenance` and the audit-retention janitor have no interval key | Owner decision 2026-11-14: new keys `identity_janitor_interval` (5 min) and `audit_retention_interval` (1 h); registered with AZ-3789's completion |

Without the first two, `ApiServices`, `HttpServer` and `serve` cannot be assembled; AC-1 is untested and the `serve` half of AC-5 is not run. AZ-3789 completes after AZ-4309, AZ-4310 and AZ-4311.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings` (and the server crate with `--features vault`), `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18: 3706 passed, 0 failed, 31 ignored. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no migration in this batch.

Warnings (recorded):
- `DeviceGrantIp` and `InvalidTokenIp` limits have no AC-20 numbers; 100/min default used.
- 04's scrub step and missing-blobs page sizes have no key; both use `gc_batch`.
- Rate-limit, db and audit metric collectors are not registered (outside this spec).
- The 12-owned doctor checks of E-22/c14b do not exist yet; `serve` start order is AZ-3791.

## Commit

`[AZ-3789] Composition root: builders and late lists` — Loop: 1

## Next Batch

AZ-4309 (07 + contracts), AZ-4310 (06), AZ-4311 (08 + 02 + contracts) in parallel; then AZ-3789's `ApiServices` and `serve` part.
