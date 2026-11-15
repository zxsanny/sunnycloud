# Batch Report

**Batch**: 75
**Tasks**: AZ-3805_media_commands, AZ-3798_scrub_commands
**Date**: 2026-11-15

Both tasks write the same files (`dispatch.rs`, `lib.rs`, the root registrations, `settings.rs`), so the parent did them one after the other, no parallel wave. AZ-3797 (key commands) was in the plan and stays in `todo/`: see Blocked.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3805_media_commands | Done | `media_commands.rs`, `dispatch.rs`, `root/media.rs`, `Cargo.toml` (+`uuid`, dev `async-trait`) | 4/4 + 1 root test | None |
| AZ-3798_scrub_commands | Done | `scrub_commands.rs`, `dispatch.rs`, `settings.rs`, `root/storage.rs`, `outcome.rs` | 6/6 + `scrub_sigint` 1/1 + 1 root test | None |

AZ-3805: `admin media-requeue [--account] [--kind]` and `admin media-stats` through `dyn MediaAdmin`; filters are validated before any service is built (exit 3, no call); the admin service is built once (`MediaAdminService`) and shared with `Media`, so the command needs neither the tree nor the blobs. AZ-3798: `scrub run [--full] [--rate]` and `scrub status` through `dyn Scrubber` (04's instance): `start_run`, `run_step` until `done`, one `finish_run` also after a failed step or SIGINT, progress on stderr every `cli_progress_interval` (new `CliSettings::progress_interval`), exit 2 on any mismatch or missing blob, 130 on a clean run cut short by SIGINT.

## Blocked

AZ-3797 `key-check`, `admin keycheck-init`, `admin rewrap`: the spec's flagged concern (Medium, open) is not answered: `MasterKeyring::rewrap_batch(from_id, to_id, n)` works on key ids of the configured ring, while the command takes `--new-key-file`; how the file becomes a ring id, and the batch size `n`, are not stated. Guessing one would be a stand-in. Needs an owner or E-10 answer.

## Code Review Verdict: PASS_WITH_WARNINGS

Warnings (recorded, none blocks):
- The job-kind names (`stills`, `poster`, `transcode`) are a three-entry table in 12: 08's name helpers are crate-private and typed with `media_shared`, not `contracts`. A fourth job kind needs the table edited (spec concern, accepted-risk, Low).
- `scrub status` prints `oldest_scrubbed_at_micros` as Unix microseconds: `sc_clock::Timestamp` has no formatter. Later commands that print times (`admin audit`, `admin invite list`) will need one shared formatter.
- A clean run cut short by SIGINT exits 130 (the spec left it open); findings win over 130.
- `composition_root.rs` is at 486 lines; the next test there needs a split.

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Test Suite

- Total: 3788 tests
- Passed: 3757
- Failed: 0
- Skipped: 31 (ignored, unchanged)

`cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18 (`sunnycloud-pg18`, `max_connections=400`).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Exit code of a `scrub run` cancelled by SIGINT | AZ-3798 concern 3 | 130 when no finding, 2 when there is one | unclear (owner may prefer 0 or 2) |
| 2 | `Scrubber::status` has no per-blob findings or suspect set, so `scrub status` prints counts only | AZ-3798 concerns 1, 2 | Counts only | clear (contract) |
| 3 | No timestamp formatter in `sc_clock` | AZ-3798 AC-5 | Unix microseconds in the output | unclear (a shared formatter is a small helper task) |
| 4 | A step failure in `scrub run` still calls `finish_run` | AZ-3798 AC-4 risk 1 | Yes, so a summary is written for the partial run; a failure of that call is logged | clear |
| 5 | `rewrap_batch` takes key ids, the command takes a file | AZ-3797 concern 1 | Blocked, ask | unclear |

## Commit

`[AZ-3805] [AZ-3798] Media and scrub operator commands` — Loop: 1

## Next Batch: remaining `todo/` tasks in dependency order (AZ-3807 doctor aggregation, AZ-3794 identity commands, AZ-3822 admin dev seed)
