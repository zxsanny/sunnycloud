# Batch Report

**Batch**: 65
**Tasks**: AZ-3759_transfer_perf_metrics (done; PT rows `NOT-RUN(HW-REF)`), AZ-3699_share_password_access_grant (done), AZ-3818_last_admin_guard (done), AZ-3822_admin_dev_seed (partly: stays in todo)
**Date**: 2026-11-14

Owner decisions 2026-11-14 applied: AZ-3699 stored PHC parameters, concurrency 4, a link without a password answers `NotFound`; AZ-3818 row locks with one retry; AZ-3822 accounts through invites and a provider sign-in. Three parallel workers: 03 (AZ-3759), 06 (AZ-3699), 01 + 07 (AZ-3818 then AZ-3822). The parent moved `hmac` 0.13 into the workspace dependency table (06 declares `hmac = { workspace = true }`), moved the default 4 of `share_password_max_concurrency` into `sc_config`'s key table (06's `DEFAULT_PASSWORD_MAX_CONCURRENCY` removed; the field is no longer optional), updated the config helper doc, removed the worker's release build folder, reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3759_transfer_perf_metrics | Done | All 8 transfer series were already emitted; added a label-set unit test and a mixed-run scrape test (AC-1), a structural AC-3 test (1 MiB and 16 MiB commits make the same blob I/O: one head read <= 64 KiB, no writes). PT-03-1..4 written in a `perf` binary and `#[ignore = "NOT-RUN(HW-REF)"]` (never run here; the harness is unvalidated until REF-SRV). PT-03-5 download half in `download_memory` (ignored by default; by hand in release: 5 GiB, heap peak +350 KB, RSS +78 KB). `observability.md` `commits_total` kinds corrected (`block_list`, `batch`) |
| AZ-3699_share_password_access_grant | Done | `verify_password`: lookup first (not a live password link -> `NotFound`, no permit, no hash), then a semaphore permit moved into the blocking task, Argon2id verify with the stored parameters. Grant = base64url(HMAC-SHA256(`share_grant_key`, `token_hash` ‖ `expires_at` µs)), 43 characters, constant-time check; nothing stored. `authorize_content` accepts a valid grant for a password link only. AC-1..AC-7 PASS (50 concurrent guesses peak exactly 4; >1000 mutated grants refused; near-valid vs invalid timing gap <= 1 ms) |
| AZ-3818_last_admin_guard | Done | `guard_removal` in 07 locks the active admins (01's existing `lock_active_admins`, now in a savepoint; lost lock race retried once inside the port) before demotion, disabling and both deletion paths. IT-07-3..IT-07-5 and ST-07-3 PASS (100 rounds of mutual demotion: exactly one passes; with `FOR UPDATE` removed the test fails) |
| AZ-3822_admin_dev_seed | Partly | `DevSeedImpl`: refuses production before any port call; idempotent; admin via setup token, two users via invites, each through `SignInService::begin` -> provider -> `complete`; folders with Unicode and mixed-case names, a JPEG via `stage_small` + `commit_batch` with a share link, a 1 GiB quota, an open upload. Not reachable: media jobs for seeded photos (08's enqueue effect is per account; 03's effect list is fixed — composition decision, E-16/E-22) and a real development provider (mock-idp, E-31/c13, does not exist). Spec stays in `todo/` with both blockers |

## Parent changes

- Workspace `Cargo.toml`: `hmac = { version = "0.13", default-features = false }` (already built in the workspace through sqlx and hkdf).
- `sc_config` table: `share_password_max_concurrency: Count = dflt("4", A)`; 06 config and tests follow.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3591 passed, 0 failed, 28 ignored. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no sqlx macros, no migration in this batch.

Warnings (recorded):
- 01 `set_state` on a `deleting` account answers `Unexpected` (a disable racing a self-deletion gets a 500); owner E-15/E-17 to decide a typed refusal.
- Every account deletion now takes `FOR UPDATE` on the admin rows (short wait behind an admin's in-flight change).
- The PT-03 harness is unvalidated until REF-SRV runs it; PT-03-3 stages 10 rounds of 1,000 (production `max_staged`) — owner may prefer 10 accounts or a raised cap.
- PT-03-5 download needs a `--release --ignored` step in the CI env C job.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | `hmac` not in the workspace table | AZ-3699 | Moved by the parent | clear |
| 2 | Concurrency default lived in 06 only | AZ-3699 | Moved to `sc_config` by the parent | clear |
| 3 | `authorize_derived` still missing | AZ-3699 | `PublicShareService` stays inherent methods | clear |
| 4 | `lock_active_admins` already existed | AZ-3818 | Used; retry added in the port | clear |
| 5 | `set_state` on `deleting` is `Unexpected` | AZ-3818 | Warning above | unclear |
| 6 | Seed media jobs not reachable | AZ-3822 AC-1 | Composition decision (E-16/E-22) | unclear |
| 7 | mock-idp missing | AZ-3822 | E-31/c13; test drives 01's in-process provider | clear |
| 8 | Transfer metrics already complete | AZ-3759 | Tests added instead | clear |
| 9 | Debug-build cipher too slow for PT-03-5 in the default run | AZ-3759 | `NOT-RUN(time)` by default; release by hand | unclear |

## Commit

`[AZ-3759] [AZ-3699] [AZ-3818] [AZ-3822] Transfer metrics, share grants, last admin` — Loop: 1

## Next Batch

AZ-3927 (02, 03, 06; owner: one task, `vault_file_too_large` 413), then the cumulative review of batches 64-66.
