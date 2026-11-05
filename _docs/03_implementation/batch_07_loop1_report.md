# Batch Report

**Batch**: 7
**Tasks**: AZ-3784_sc_audit, AZ-3852_decoder_protocol_module, AZ-3957_upload_planner
**Date**: 2026-11-05

Three parallel workers on disjoint paths. The Jira connector was not available this batch: tracker writes are recorded in `_docs/_process_leftovers/2026-11-05_jira-transitions-batch6.md` (tracker: pending).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3784_sc_audit | Done | crate (8 src, 6 test files), audit migration pair, `check-audit-sql.sh` and its test, 1 ci.yml step, sc_db tests updated for the migration list | 7 unit + 3 doctests + 21 integration on real PG 18; 18 seeded violations for the SQL check | Scope, purge, mirror-line points below |
| AZ-3852_decoder_protocol_module | Done | media-shared (frame, message, ranges, session, violation modules; 47 corpus files; standalone fuzz package) | 84 tests incl. deterministic fuzz over the corpus | Range bounds chosen by the worker; cargo-fuzz harness not built |
| AZ-3957_upload_planner | Done | sync-core-proto `planner` module (2 src, 3 test files) | 28 reaction tests, property tests (20 seeds x 1000 runs), cost test | Attempt-counting rule chosen by the worker |

## Code Review Verdict: PASS_WITH_WARNINGS

Focused review of the joined tree: 6 findings, all skipped with reasons (caller-controlled purge cutoff, mirror log line before commit, nil-account scope, invented range table, unbuilt fuzz harness, attempt counting). The workspace lint-inheritance tests failed on the new standalone fuzz package; fixed by letting a standalone package set the same two denies itself.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, wasm32 builds of `sync-core-wasm`, `sc_ids`, `sc_names`, `sync-core-proto`, `cargo deny check advisories bans licenses sources`; from the root `check-pinning.sh`, `check-no-sql-now.sh`, `check-db-gates.sh`, `check-audit-sql.sh`. The two lint tests were re-run after the fix.

## Test Suite

- Total: 562 (workspace, default features, real PostgreSQL 18)
- Passed: 562
- Failed: 0
- Skipped: 0

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `audit_log_purge(cutoff)` is SECURITY DEFINER with a caller-supplied cutoff; the API role can purge everything with a future cutoff | AZ-3784 | Owner decides a minimum-age rule or a separate janitor role | unclear |
| 2 | `record`, `purge`, `export` run with `Scope::Account(nil)`; `run_system` needs an allowlist entry in `scripts/ci/run-system-allowlist.txt` | AZ-3784 | Owner decision on the system-scope rule | unclear |
| 3 | The mirror log line is emitted before the caller's commit | AZ-3784 AC-6 | Accept, or emit after commit | unclear |
| 4 | `audit_retention = 0` means purge does nothing; `AuditPurge::new(db, retention)` is not wired to `Settings` | AZ-3784 | Composition root passes it | clear |
| 5 | GRANT by a role without grant option is a PostgreSQL warning, not an error; test asserts the unchanged grants | AZ-3784 AC-1 | Spec wording fix | clear |
| 6 | `sc_observability` does not exist yet; the audit line has target `audit` without a `stream` field | AZ-3784 | E-09 adds `"stream":"audit"` | clear |
| 7 | Decoder protocol decisions for the contract: big-endian u32 prefix, envelope `{"request"/"response"/"event"}`, durations in ms, `build_timestamp` Unix seconds, the full range table, session rules, queued second connection | AZ-3852 | Copy the module rustdoc into `decoder_protocol.md` | clear |
| 8 | Range bounds (probe dimensions, 7-day durations, `scale_to_short_edge` 1..=16383) are the worker's numbers | AZ-3852 | Confirm against DS-03 and DS-08 | unclear |
| 9 | `hello` is not required first; an idle `cancel` is a no-op; supervisor-side rules are not in `Session` | AZ-3852 | E-19 and E-20 confirm | unclear |
| 10 | Limits above runner ceilings are accepted; the runner must clamp before any `Duration` arithmetic | AZ-3852 AC-5 | E-19 | clear |
| 11 | cargo-fuzz harness in `media-shared/fuzz/` was not built or run (nightly, libfuzzer-sys) | AZ-3852 AC-7 | Run on a nightly host | clear |
| 12 | Planner: new terminal `Action::GiveUp{i}`; every `SendBlock` counts as an attempt; `Wait`, conflict and `on_session` semantics; stray inputs ignored | AZ-3957 | Record in the contract; AZ-3958 maps give-up to a typed error | unclear |
| 13 | Out-of-range index or empty `NeedBlocks` ends in `RestartSession`; whether the executor re-hashes first is open | AZ-3957 | AZ-3958 | unclear |
| 14 | G-03-1 (16 MiB worst-case resend vs 10 MiB) untouched; planner takes `lanes_max` as input | AZ-3957 | Owner (open since decompose) | unclear |

## Commit

`[AZ-3784] [AZ-3852] [AZ-3957] audit writer, decoder protocol, upload planner` — Loop: 1

## Next Batch: AZ-3958 sync_typed_errors (needs AZ-3957), AZ-3970 scripted fake server, AZ-3888 stub daemon UI driver, AZ-3786 sc_observability (needs AZ-3784, AZ-3785), then AZ-3768 contracts part 1
