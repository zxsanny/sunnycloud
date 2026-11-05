# Batch Report

**Batch**: 5
**Tasks**: AZ-3766_sc_ids, AZ-3767_sc_names, AZ-3782_sc_db_access, AZ-3785_sc_ratelimit
**Date**: 2026-11-05

Four parallel workers on disjoint crates; `Cargo.toml`/`Cargo.lock` updates were serialised with a file lock. The parent restored `BSD-3-Clause` and `Zlib` to the licence allow-list in `src/deny.toml` (removed by the parent in batch 2 as unused, both GPL-compatible), because `subtle` (sc_ids) and `foldhash` (sqlx) need them.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3766_sc_ids | Done | crate (6 src, 4 test files, vector file), root Cargo.toml | 20 tests, wasm32 build, vectors recorded with an independent Python BLAKE3 | Commit-id key material and UUID version chosen by the worker |
| AZ-3767_sc_names | Done | crate (10 src, 9 test files, corpus json) | 59 tests, wasm32 build; ICU4X 2.4.0 (Unicode 18.0.0) | `sc_clock` brings tokio into the tree |
| AZ-3782_sc_db_access | Done | crate (7 src, 8 test modules), `check-db-gates.sh`, allowlist, ci.yml step | 27 tests + 2 doctests against real PG 18; gates script test | RLS policy form in data model; tls-none |
| AZ-3785_sc_ratelimit | Done | crate (4 src, 3 test files) | 28 tests | Two limits have no numbers |

## Code Review Verdict: PASS_WITH_WARNINGS

Focused review of the joined tree: 6 findings, all skipped with reasons (owner decisions, doc fixes, or follow-up design).

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, wasm32 build of `sync-core-wasm`, `sc_ids`, `sc_names`, `cargo deny check advisories bans licenses sources`; from the root `check-pinning.sh`, `check-no-sql-now.sh`, `check-db-gates.sh`. The coverage gate (`check-coverage.sh`) was not run locally (cargo-llvm-cov is not installed here); CI runs it.

## Test Suite

- Total: 301 (workspace, default features, real PostgreSQL 18)
- Passed: 301
- Failed: 0
- Skipped: 0

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The policy expression `current_setting('sunnycloud.account_id', true)::uuid` raises 22P02 on reused connections; use `NULLIF(current_setting(...), '')::uuid` | AZ-3782 AC-2 | Fix `data_model` 12.3 and use the NULLIF form in migrations (E-06/c01b onward) | clear |
| 2 | `DbError::WrongRole(&str)`, `Other(String)`, `UniqueViolation.constraint: Option<String>`, `25P03 -> Unavailable`, `DbTx::conn()` differ from or extend the contract | AZ-3782 | Update `contracts/shared_db` | clear |
| 3 | sqlx built with `tls-none`; no `sslmode=require` | AZ-3782 | Add TLS when a remote database is supported | unclear |
| 4 | "Repository module" defined by the gate script as `repo`, `repository`, `repositories` dirs/files or `*_repo.rs`; `run_system` allowlist is file-level in `scripts/ci/run-system-allowlist.txt` | AZ-3782 AC-10 | Component owners confirm | unclear |
| 5 | Commit-id derivation: `derive_key("sunnycloud commit v1", b"")` keyed hash over device id, asset id, file hash; UUID version 8; frozen by vectors | AZ-3766 AC-3 | Owner confirms before Kotlin/Swift copy the vectors | unclear |
| 6 | `new_id` panics without a random source (contract has no error path) | AZ-3766 | Contract change to return `Result` if wanted | unclear |
| 7 | `TokenKind` has the eight families; no token prefix strings were specified | AZ-3766 flagged concern | Owner | unclear |
| 8 | WASM known-answer test belongs in `sync-core-wasm` (no wasm-bindgen in sc_ids); `getrandom` `wasm_js` pulls wasm-bindgen transitively | AZ-3766 | Add when E-23 fills the wasm crate | clear |
| 9 | `sc_clock` pulls tokio into `sc_names`; AZ-3767 AC-6 says no async runtime dependency | AZ-3767 AC-6 | Split `Timestamp` from tokio in sc_clock (feature or crate) or let the contract take microseconds | unclear |
| 10 | ICU4X (`icu_normalizer`, `icu_casemap` 2.4.0) chosen over `unicode-normalization` named in the E-13 epic; `NAME_KEY_VERSION = 1800` | AZ-3767 | Update epic and tree description | clear |
| 11 | Escaped-name collision suffix ` (2)` goes at the very end; conflict copies put it before the extension | AZ-3767 | Owner (`%5F` marker and suffix already an owner item) | unclear |
| 12 | `DeviceGrantIp` and `InvalidTokenIp` limits have no numbers or windows; `LimitTable` has no `Default` | AZ-3785 | Owner supplies numbers; composition root passes them | unclear |
| 13 | `LimitTable` carries `ratelimit_max_entries` (default 100,000) because the contract's `new(clock, table)` has no argument for it | AZ-3785 | Contract update | clear |
| 14 | Effective bucket cap is slightly below `max_entries` (per-shard caps) | AZ-3785 AC-5 | Accept | clear |

## Commit

`[AZ-3766] [AZ-3767] [AZ-3782] [AZ-3785] ids, names, db access, ratelimit` — Loop: 1

## Next Batch: AZ-3783 sc_db_migrator_accounts_base (needs 3782, 3740, 3741), AZ-3784 sc_audit, AZ-3786 sc_observability (needs 3783, 3784, 3785), AZ-3768 contracts part 1 (needs 3786)
