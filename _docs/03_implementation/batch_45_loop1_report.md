# Batch Report

**Batch**: 45
**Tasks**: AZ-3875_worker_offset_clock_poll, AZ-3856_decoder_safe_open_watchdog, AZ-3922_vaults_migration_config
**Date**: 2026-11-12

Three parallel workers on disjoint crates (`worker`, `decoder`, `20_vault_service` plus one new migration). The parent owned `Cargo.lock`, the cross-crate test harness, the review fixes, the gates and the commit. The batch was ready because the spikes of batches 38 to 44 unblocked it. Before the batch the test database container `sunnycloud-pg18` was restarted (it stopped in the host restart of 2026-11-11 21:36, see batch 44's repair note).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3875_worker_offset_clock_poll | Done | `worker`: new `api/clock.rs`, `clock_poll.rs`, `clock_source.rs`; `run.rs`, `compose.rs`, `lib.rs`, `api/mod.rs`, `Cargo.toml` (`test-clock` enables `sc_clock/test-clock`); tests `api_clock.rs`, `clock_poll.rs`, `clock_poll_release.rs`, `process_support/`, `wiring.rs`, `slot_support/*`, `supervisor_support/mod.rs` | AC-1: the real worker binary follows the API offset by whole days (`image_age_days` 20, 20 after 1 h, 23 after 3 d 1 h), bearer and worker id on every poll, at most one poll a second; AC-2: the release binary makes no offset request and holds no `OffsetClock` or route string | Items 1-4 |
| AZ-3856_decoder_safe_open_watchdog | Done | `decoder`: new `safe_open.rs`, `watchdog.rs`, `test_freeze.rs`, `sys.rs`, `scripts/check-no-test-freeze.sh`; `config.rs` (`scratch_root`, `watchdog`, write timeout half the watchdog), `connection.rs`, `server.rs`, `job.rs` (`JobContext::input`), `main.rs`, `lib.rs`, `Cargo.toml` (`libc`, feature `test-freeze`); tests `safe_open.rs`, `self_abort.rs`, `watchdog.rs`, `release_check.rs` and the existing suites on a real scratch tree | AC-1: FIFO, symlink, hard link, directory and device refused as `Corrupt` within 1 s, a control job runs after each, also through the real binary; AC-2: job paths outside the scratch root and symlinks at every position refused; AC-3: SIGUSR1 makes the real binary abort with SIGABRT after 3.2 s idle and 3.1 s busy; AC-4: `strings`/`nm` find no `test_freeze` in the release binary, the seeded build is caught; AC-5: a 30 s scripted job with a 3 s watchdog never aborts | Items 5-8 |
| AZ-3922_vaults_migration_config | Done | new `src/migrations/20261112100000_create_vault_table.{up,down}.sql`; `20_vault_service`: `Cargo.toml`, `src/lib.rs`, new `src/config.rs`, tests `tests/vault/*` | 20 tests on PostgreSQL 18: six columns, `account_id` and `root_node_id` unique, the composite foreign key to `nodes`, no `vault_files`, up/down/up, two-account row-level isolation, DML-only grants, the config defaults (104,857,600 and 1024) and the refusals naming the key | Items 9-11 |

## Cross-crate and lockfile changes (parent)

- `Cargo.lock`: `decoder-runner` gains `libc`; `sunnycloud-vault-service` gains `sc_config`, `sc_db`, `sc_testdb`, `sqlx`, `tokio`, `uuid`. No new package and no new version.
- The worker's decoder test harness (`tests/decoder_support/runner.rs`) runs the real runner on a real scratch tree (`job/in/source`) because the runner now checks every job directory before its handler; `decoder_calls.rs` sends that path.
- The decoder's hand-declared `prctl`/`signal` and hard-coded flag numbers were replaced by `libc` (already in the lock; precedent in the desktop crates).

## Code Review Verdict: PASS_WITH_WARNINGS (after fixes)

Six findings. Fixed with the owner's approval: the decoder write timeout equalled the watchdog limit (now half of it, so a stuck write ends the connection before the watchdog aborts the runner); the worker id was resolved twice when unset, so the clock poll sent a different `X-Worker-Id` (now resolved once in `serve` and passed to both clients); `VaultConfig::from_settings` returned an unvalidated config (now validates and returns `ConfigError::Invalid`). Fixed as mechanical: the clock poll logged the same warning every second (now once per new problem). Skipped: the 1024 default of `max_vault_header_bytes` belongs in `sc_config`'s key table (item 9). No change: `libc` instead of `rustix` in the decoder (`rustix` cannot install the test-only signal handler; `libc` is a workspace precedent).

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids` and `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh` exit 0; `cargo test --workspace --locked --no-fail-fast`: 2806 passed, 1 failed, 9 ignored. The one failure was the new worker release check: under `cargo test --workspace` feature unification builds `sc_clock` with `test-clock` (its own dev-dependency), so `OffsetClock` appears in the debug info of every test-profile binary. The check now looks for the worker's own gated markers (`follow_api_offset`, the route string); re-run in workspace mode: 2 passed; positive control with the feature on finds both markers (801 and 1 occurrences). The crate suites: decoder 53 / 56 with `test-freeze`, worker 211 / 220 with `test-clock`, vault service 20, all passed. Not run: `check-migrations.sh` (needs `psql` and `sqlx-cli`, not installed) and `cargo deny` (known RUSTSEC-2026-0459).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | The poll interval has no config key | AZ-3875 flagged concern | Constant 1 s (helper 03: at most once a second) | unclear |
| 2 | A failed poll: warn and retry next tick; a 403 ends the poll (the claim loop reports the refused secret) | AZ-3875 | As described | unclear |
| 3 | The API offset can go back when a test resets it; the offset clock only grows | AZ-3875 | `WentBack` is logged once and the old offset kept; the supervisor needs a restart | unclear |
| 4 | The first status report can go out before the first poll and show offset 0 | none | Left; the next report follows | clear |
| 5 | `RLIMIT_CORE=0` did not stop the host core handler (apport pipe) and delayed the abort by about 1 s | 10 section 13.6 | `prctl(PR_SET_DUMPABLE, 0)` at start instead; overlaps the PR_SET_DUMPABLE item of E-19/c07a | unclear |
| 6 | A job directory refusal answers `Corrupt`, not `ProtocolViolation`, so the runner keeps serving | AZ-3856 | As described | clear |
| 7 | The watchdog aborts without a log line: the stuck loop may hold the logger | AZ-3856 | The SIGABRT exit status is the record | clear |
| 8 | AC-5 ran with a 30 s scripted job, not a real transcode: no executors exist yet | AZ-3856 AC-5 | Repeat with the transcode executor (E-19/c11) | unclear |
| 9 | `sc_config` declares `max_vault_header_bytes` with no default, so the vault crate applies 1024 | AZ-3922 AC-5 | Follow-up: `dflt("1024", A)` in `shared/config/src/table.rs` | unclear |
| 10 | The vault root foreign key has no delete rule: a vault root cannot be purged while the vault exists | AZ-3922 risk 1 | Pinned by a test; account erase runs 20's hook before 02's | unclear (owner) |
| 11 | `account_id` UNIQUE added beyond the spec, following the one-vault-per-account decision; `nodes.vault_id` is not in this task | AZ-3922 | Separate follow-up task (data model owner) | clear |

## Commit

`[AZ-3875] [AZ-3856] [AZ-3922] Worker clock poll, decoder safe open and watchdog, vaults table` — Loop: 1

## Next Batch

Ready next: AZ-4007 (vault-core skeleton), AZ-3908 (desktop token store and device login), AZ-3693 (identity scope declarations check), AZ-3793 (`migrate` commands). Five-point tasks wait for the owner's complexity-budget choice: AZ-3708, AZ-3747, AZ-3834, AZ-3916.
