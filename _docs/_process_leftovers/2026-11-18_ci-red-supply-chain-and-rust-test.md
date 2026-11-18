# Leftover: CI is red on dev (supply-chain and rust-test), pre-existing

kind: ci triage (read-only), found 2026-11-18, red since at least 2026-11-17
evidence: `gh run view 44412660514 --log-failed` (and every dev push since AZ-3807); all other jobs pass (rust-lint, pg18-migrations, http-api-suite, images, pipelines-large, vault-core x3)

1. `supply-chain` (audit.sh): RUSTSEC-2026-0459 on `libsqlite3-sys 0.37.0` ("upgrade to >=0.39.0"; find which crate still pulls 0.37.0 into `src/Cargo.lock`, the workspace pins `rusqlite =0.39.0`) plus 5 denied warnings for unmaintained crates: basic-toml 0.1.10 (RUSTSEC-2026-0374), chrono 0.4.45 (-0543), zbus_names 4.3.5 (-0371), zvariant 5.15.1 (-0370), zvariant_derive (-0372). Fix by upgrading or removing the edges, or by reviewed time-boxed ignores in `scripts/ci/audit_ignores.py` (owner policy decision).
2. `rust-test` (`check-coverage.sh` under `watchdog.sh --name rust-test`, ci.yml ~line 87): watchdog `STALLED ... no output for 120s (exit 124)` at about 218 s, right after `Compiling sync-core-wasm`: the instrumented workspace compile under `cargo llvm-cov` is quiet for longer than the 120 s default. Fix: `WATCHDOG_STALL=600` for that step only (as the mobile setup step does) or make the step print progress. The coverage thresholds (75% overall, 90% critical) and the test results themselves have not been seen on CI since the stall hides them.

Not caused by the AZ-3985..AZ-4000 batches (the same two jobs failed on AZ-4334 and earlier). The `mobile` job, which the AZ-3997/AZ-4002 push introduced, was still running when this was written. The autodev session tried to propose a fix session; the app hook timed out, so this file is the record.
