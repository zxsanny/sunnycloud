# Batch Report

**Batch**: 1
**Tasks**: AZ-3652_initial_structure
**Date**: 2026-11-05

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3652_initial_structure | Done | 75 files (workspace root, 32 crates, crate-graph and hygiene tests) | 8 integration + 1 doctest pass | None |

Not in this batch: the eight other tasks with no dependencies (AZ-3681, 3695, 3705, 3721, 3722, 3731, 3732, 3773) are spikes that need owner decisions or measurements, so they were not guessed.

## Code Review Verdict: PASS_WITH_WARNINGS

5 findings (path separators, repeated `cargo metadata`, weak doctest, unused licences, no lint-inheritance test), all fixed.

CI-parity: PASS. Commands in `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `cargo deny check advisories bans licenses sources`.

AC check: AC-1..AC-5 and AC-7 verified by the commands above; AC-2 seeded `let _ = f();` rejected by clippy (manual); AC-6 seeded tokio 1.43.0 and h2 0.4.15 rejected by `cargo deny check bans` in a scratch copy.

## Test Suite

- Total: 9 (8 integration tests in `sunnycloud-server`, 1 doctest in `sc_db`)
- Passed: 9
- Failed: 0
- Skipped: 0

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | cargo-deny ban syntax `crate = "name@<req>"` is silently ignored; `name:<req>` works (0.20.2) | AZ-3652 AC-6 | `deny.toml` uses `name:<req>`; the supply-chain CI job must run the same cargo-deny version | clear |
| 2 | tokio 1.54.0 is yanked, so the AC-6 branch check was run with tokio 1.43.0 | AZ-3652 AC-6 | none needed; the ban covers `< 1.54.1` | clear |
| 3 | `cargo-deny` was not installed on the workstation | AZ-3652 AC-6 | installed with `cargo install cargo-deny --locked`; CI skeleton must install a pinned version | clear |
| 4 | Toolchain pinned to 1.97.1 while `rust-version` stays 1.88; the MSRV is not built anywhere | AZ-3652 | CI skeleton (AZ-3761) may add an MSRV build job | unclear |

## Commit

`[AZ-3652] Scaffold Cargo workspace and skeleton crates` — Loop: 1

## Next Batch: AZ-3739, AZ-3740, AZ-3741, AZ-3744, AZ-3763, AZ-3764 (dependencies on AZ-3652 now met)
