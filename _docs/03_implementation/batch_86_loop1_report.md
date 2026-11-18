# Batch Report

**Batch**: 86
**Tasks**: AZ-3926_vault_routes_registration, AZ-3824_admin_performance (parallel wave, two workers; parent review and fixes)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3926 | Done | 11 `registry/vault/` (new), `registry/mod.rs`, `server.rs`, `port_error/services.rs`, `lib.rs`; `sc_error` codes; sync-core wire table; 20 `service.rs`, `vault_repo.rs`; server `dispatch.rs`; tests in 11 (`it/vault`, `it/registry/vault.rs`, `it/matrices/{vault,gates_vault}.rs` and matrix plumbing), 20 (`service_root.rs`), server (`vault_http.rs`, `vault_report.rs`, `vault_support/`, `local_commands.rs`); `base64` (workspace pin) in 11 and as a server dev-dependency | All pass | 1 defect in 20 found and fixed (see Discovered #1) |
| AZ-3824 | Done (timing rows NOT-RUN(HW-REF)) | 07 `tests/admin_perf/` (new: `main`, `load`, `listing`, `purge_load`, `background`), `tests/purge_support/{world,probes}.rs`, `tests/purger/budget.rs` | 4 pass, 2 ignored NOT-RUN(HW-REF); functional 3x stable | Existing IT-07-11 sampler bug found and fixed (see Discovered #3) |

Owner decisions of 2026-11-18 (asked in this session) closing `vault_http_api.md` open points: header as standard base64 in JSON; strong quoted lowercase hex SHA-256 `ETag`; `HeaderTooLarge` 413 `vault_header_too_large`, `VaultExists` 409 `vault_exists`; unlock-failure report 204. New `sc_error` rows also 412 `precondition_failed` and 428 `precondition_required`. Contract `vault_http_api.md` 1.1.0 frozen; `sc_error.md` 1.1.0; `api_v1_surface.md` 1.9.0.

AZ-3926 mechanics: routes are mounted and documented only when 12 fills 11's vault slot (`served_routes(&ApiServices)`), so v1.0/v1.1 builds have no vault route, document entry, service or hook (AC-7, both builds tested). `If-Match`: absent gives 428 with no port call; exactly one quoted tag reaches the port; anything else present (empty, unquoted, weak, `*`, a list) reaches the port as an empty tag and answers 412. `sunnycloud openapi` prints `vault_document()` in a `vault` build.

AZ-3824: PT-07-1 (500 accounts, 4 callers at 20 calls/s) and PT-07-2 (purge at batch 5000 while 20 accounts commit through 03's real `stage_small` + `commit_batch`) each have a functional CI test (0 errors, page contents, at most 4 active sessions, 2 data statements per page; nothing of the account left, files gone, one `account_deleted` row, 2-connection purger pool, all other commits succeed) and a full-scale test ignored as `NOT-RUN(HW-REF)` that prints the host line and per-metric verdicts. Leftover: `_docs/_process_leftovers/2026-11-18_az-3824-ref-srv.md`.

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | High | Security | `20_vault_service/src/service.rs` `create_vault` | `ON CONFLICT DO NOTHING` read every unique conflict as `VaultExists`: B naming A's vault root got 409, an unknown root 404, so B could tell that A's folder is a vault root (AC-4 / AC-59) | **Fixed**: on a conflict, `VaultExists` only when the caller already has its vault, else `NotFound`; 20 test `another_accounts_vault_root_is_not_found_like_an_unknown_root`; the worker's real-API test now passes; contract wording updated |
| 2 | Medium | Spec-Gap | `server/src/dispatch.rs` | `sunnycloud openapi` printed the v1.0 document in a `vault` build (AC-7) | **Fixed**: `vault_document()` under feature `vault`; `local_commands` asserts the vault path presence per build |
| 3 | Low | Maintainability | `07_admin/tests/admin_perf/{listing,purge_load}.rs` | Full-scale tests also asserted the targets, so `TARGET-MISSED` failed the run; the 06 precedent fails only above the failure threshold and PT-07-1's targets are provisional | **Fixed**: target asserts removed; verdict lines still print |
| 4 | Low | Build | `sync-core/src/wire_table.rs` | The four new `sc_error` codes broke the exhaustive wire table | **Fixed** (parent): added to the `unexpected` arm, as AZ-3927 did for earlier codes |

## Test Suite

- `sc_error` all pass; `sync-core --lib` 25; `sunnycloud-http-api` 86 + 361 (1 ignored, pre-existing) + 1; `sunnycloud-vault-service` 3 + 45; 07 `admin_perf` 4 (2 ignored NOT-RUN(HW-REF)), `purger` 10.
- Server, default build (`api_services`, `composition_root`, `crate_graph`, `doctor_findings`, `local_commands`, `user_commands`, `vault_http`, `vault_report`): 51 passed. Server, `--features vault` (`api_services`, `composition_root`, `local_commands`, `vault_http`, `vault_report`): 26 passed. 0 failures.
- CI-parity (`CARGO_INCREMENTAL=0`): first run failed two gates on this batch's test code, both fixed and re-run green: `check-no-sql-now` (`now()` in a vault test seed, now a bound `sc_clock` time) and `check-db-gates` (`run_system` in `admin_perf/purge_load.rs`; moved to the test repository module `admin_perf/pool_repo.rs`, added to `run-system-allowlist.txt`: it reads `pg_backend_pid()` of the purger pool, no account row). Final: `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm`, `check-pinning` exit 0. `CI-parity: PASS`.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | B creating a vault on A's vault root answered 409 `vault_exists` (existence leak) | AZ-3926 AC-4, AC-59 | Fixed in 20 this batch | clear |
| 2 | A header that is not base64 answers 400 `invalid_request` with field `header`/`not_base64` (the existing body-validation path), not 422 | AZ-3926 AC-1 | Kept: follows every other JSON route | clear |
| 3 | IT-07-11's longest-transaction sampler read `pg_stat_activity` as the migrator role, which cannot see the API role's `xact_start`, so it always read 0 | AZ-3820 AC-3 | Fixed: sampler on a superuser connection; `purger` 10/10 | clear |
| 4 | PT-07-2 "running server, data through the API": 07 cannot depend on the server crate | AZ-3824 AC-2 | Port-level run with real components; account bulk-seeded; other accounts commit through 03's real services; the API-level run stays with the system tier (E-31) | clear |
| 5 | PT-07-2 GC: a GC cycle runs after each purger cycle with the purge's blobs due at once, not on the 60 s `gc_interval` | AZ-3824 Risk R12 | Report separates purger and GC time; on the real GC schedule finalize may be longer | unclear |
| 6 | "100 sessions" read as sign-in sessions; the seed plan's 3 upload sessions are also present | AZ-3824 AC-2 | Kept | unclear |
| 7 | AC-4 "without A's If-Match": a missing `If-Match` answers 428 for any id before any lookup, so it reveals nothing; 404 is tested with A's tag and a foreign tag | AZ-3926 AC-4 | Kept | unclear |
| 8 | The generated matrices now take the full registry; the ignored real-stack matrix run against a v1.0 stack would list the vault operations as failing | AZ-3926 AC-4, E-21/c20 | Decide when E-21/c20 runs against a stack | unclear |
| 9 | Workstation debug numbers (not results): `list_users` p95 70-90 ms; with a 2,000-file account other accounts' commit p95 rose from ~0.4 s to 2-3 s while GC cycles took 3-9 s | AZ-3824 | Watch on REF-SRV | clear |

## Commit

`[AZ-3926] [AZ-3824] Vault routes and admin performance suite` — Loop: 1

## Next Batch: AZ-3656, AZ-3655 (in-process pipelines through the real binary), AZ-3835 (runner, crate graph and coverage gate; Mode R legs NOT-RUN until the test stack has the SUT). AZ-3971's real-server legs wait for the SUT in the test stack (E-31).
