# Batch Report

**Batch**: 71
**Tasks**: AZ-4309_admin_service_impl (done), AZ-4310_share_authorize_derived (done), AZ-4311_media_effect_account_per_commit (done); AZ-3822 media part done (spec stays in `todo/` for mock-idp)
**Date**: 2026-11-14

The three follow-ups of AZ-3789's blockers. Two parallel workers: contracts + 02 + 07 + 08 + 11 + server (AZ-4309 then AZ-4311) and 06 (AZ-4310). The parent reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-4309_admin_service_impl | Done | `AdminError::Forbidden` (CC-3); `impl AdminService for AdminServiceImpl` authorizes first, then calls crate-private bodies; `AdminCallError` and the public inherent methods removed (one path); `health` returns `Result<HealthReport, AdminError>` (only `Forbidden`; a failing contributor stays an item in the report); 11 maps `Forbidden` to 403 `forbidden`. AC-1..AC-3 PASS through `dyn AdminService` |
| AZ-4310_share_authorize_derived | Done | `authorize_derived` shares one private path with `authorize_content` (`allowed_file`: IP limit, lookup, liveness, one `resolve_live_file` read, grant check, invalid-token count on every `NotFound`); derived returns `ShareDerived{account_id, file}` with no `share_accessed` audit; `impl PublicShareService for PgShareService` for all four methods, existing suites call through the trait. AC-1..AC-4 PASS (derived NotFound cases run exactly `authorize_content`'s statements) |
| AZ-4311_media_effect_account_per_commit | Done | `PublishedEntry.account_id` filled by 02's `publish` from `CommitActor`; `MediaEnqueueEffect::new(queue, planner)` and `JobPlanner::plan(&entry)` read the account from the entry; the server's `CommitEffects` late list holds one shared 08 effect given to 03. AC-1..AC-3 PASS (two accounts each get their own job from one effect instance) |
| AZ-3822 (media part) | Done for media | The dev seed's photos now queue one stills job per user under the user's account; the mock-idp concern stays open |

## Contract changes

- Part 1 2.0.0: `AdminError::Forbidden`; `AdminService::health` returns `Result<HealthReport, AdminError>` (breaking: return type). The brief said to keep `health -> HealthReport`; the worker changed it because AC-1 needs all four operations to answer `Forbidden`, `admin_service.md` and CC-3 already list `Forbidden` for `health`, and keeping it infallible would have dropped 07's authorization on `health`. Accepted by the parent.
- Part 2 1.4.0: `PublishedEntry.account_id` (part 2 open point 3 resolved).

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18: 3713 passed, 1 failed, 31 ignored (a first gate run stopped at the test step with no test output and passed `--no-run` right after; re-run). The failure: 03 `reaper_race::session_reaper_racing_commit_upload_has_exactly_one_winner_each_round`; its message was not captured by the gate filter. It then passed 35 of 35 runs (15 alone or under extra load, 20 under the gate's load). Recorded as an unexplained, non-reproduced failure for the race suite (E-14/c11); the gate now keeps the full test output so a recurrence carries its message. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no migration in this batch.

Notes (recorded):
- Part 1 is 2.0.0 although CC-3 called the change minor (the `health` return type changed); owner to confirm.
- `authorize_derived`, like `authorize_content`, does not re-check `LiveFile.vault`; a shared file cannot enter a vault (AZ-3927 refuses moving or seeding a vault with a shared file), so no gap today.
- 07 `purge_support` and 03 `range_memory` still pass empty `commit_effects` (not media tests).

## Commit

`[AZ-4309] [AZ-4310] [AZ-4311] Admin and share ports, media effect per commit` — Loop: 1

## Next Batch

Finish AZ-3789: `ApiServices`, `HttpServer`, `serve`, and the two janitor keys (`identity_janitor_interval` 5 min, `audit_retention_interval` 1 h).
