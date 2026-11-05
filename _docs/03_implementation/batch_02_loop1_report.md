# Batch Report

**Batch**: 2
**Tasks**: AZ-3739_sync_core_skeletons, AZ-3740_migrations_setup, AZ-3741_docker_compose_skeletons, AZ-3744_license_readme, AZ-3763_sc_config, AZ-3764_sc_error
**Date**: 2026-11-05

Four tasks ran as parallel workers with disjoint paths (3740, 3741, 3763, 3764); the parent did 3739 and 3744 and owned Cargo.toml/Cargo.lock.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3739_sync_core_skeletons | Done | 5 files | 4 new tests (wasm split, single wasm-bindgen home) + wasm-pack 0.15.1 package: glue JS + one `.wasm` | None |
| AZ-3740_migrations_setup | Done | scripts/ci (2 scripts, README, 3 tests) | AC-1..AC-4 proven against PostgreSQL 18 | None |
| AZ-3741_docker_compose_skeletons | Done | deploy/** (3 Dockerfiles, dev compose, init script, 8 test files), .dockerignore | AC-1..AC-6 proven with 5 real image builds, DB start | mock-idp left out (see below) |
| AZ-3744_license_readme | Done | LICENSE, README.md, scripts/ci/licence.sha256 | `sha256sum -c` OK | None |
| AZ-3763_sc_config | Done | src/shared/config (9 src files, 6 test files) | 63 tests | 4 owner questions |
| AZ-3764_sc_error | Done | src/shared/error (7 src files, 6 test files) | 25 tests, 31 with `client` | 4 contract mismatches |

## Code Review Verdict: PASS_WITH_WARNINGS

8 findings: 1 fixed (secrets are accepted from files only), 1 no change needed, 6 skipped with a reason (owner decisions or doc follow-ups listed below).

CI-parity: PASS. In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` (and `-p sc_error --features client`), `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids` and `-p sc_error --features client`, `cargo deny check advisories bans licenses sources`.

Docker builds were run by the AZ-3741 worker; CI does not run them yet.

## Test Suite

- Total: 98 (workspace, default features) + 31 for `sc_error` with `client`
- Passed: 98 / 31
- Failed: 0
- Skipped: 0

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Release-fixed config rows (`updater_source`, `updater_public_key_1`, `updater_public_key_2`, `min_api_family`, `https_required`) are not in helper 01's table; names were chosen by the worker | AZ-3763 AC-6 | Owner confirms names; add rows to `01_helper_config.md` | unclear |
| 2 | 15+ keys have no default, type or range in the helper table (listing_max_limit, device_grant_ip_limit, blob_root, share_argon2_params, ...); they are optional free values | AZ-3763 | Owner/epic owners supply values; no stand-in invented | unclear |
| 3 | Only 13 of the duration keys have floor/ceiling in the table; the rest are unbounded | AZ-3763 | Complete the table | unclear |
| 4 | `public_url` must be https outside development, so `http` is refused in `environment = test` too | AZ-3763 | Confirm that tests use https or `development` | unclear |
| 5 | `reload_client_settings` is a method on `ClientSettingsHandle`; the crate has no logger, so the binary must log the active `environment` | AZ-3763 | Contract text update; E-09/E-22 log it | clear |
| 6 | `ApiError::Api` carries `params`, not only `retry_after`; transport input is status 0; `problems` field added to the body | AZ-3764 | Update `contracts/shared_error/sc_error.md` | clear |
| 7 | The db container must mount `db_api_url` and `db_migrator_url` for the role init script (roles' passwords come from them) | AZ-3741 flagged concern | Update `containerization.md` and the release compose in Step 13 | clear |
| 8 | `mock-idp` has no source (`tests/mocks/idp` does not exist), so the dev compose has no sign-in provider | AZ-3741 | The task that builds the mock adds the service | unclear |
| 9 | `TEST_CLOCK` build arg added next to `FEATURES` (a LABEL cannot derive from a feature); a guard fails the build on a mismatch | AZ-3741 AC-2 | Keep, or pass the label from CI | unclear |
| 10 | Host has no buildx; images built with the legacy builder. Dockerfiles set `RUSTUP_TOOLCHAIN=1.97.1` to skip component downloads | AZ-3741 | CI skeleton states the builder; update the env var with the toolchain pin | clear |
| 11 | `check-migrations.sh` needs an empty database and leaves it migrated | AZ-3740 | Documented in `scripts/ci/README.md`; CI uses a fresh service | clear |
| 12 | `check-sqlx-prepare.sh` needs a database with all migrations applied | AZ-3740 | CI skeleton (AZ-3761) places it in a job with the PG 18 service | clear |
| 13 | tokio 1.54.0 is yanked; cargo-deny ban syntax `name@<req>` is ignored, `name:<req>` works (batch 1 note) | AZ-3652 | Already applied | clear |
| 14 | wasm-pack 0.15.1, cargo-deny 0.20.2 and sqlx-cli 0.9.0 are installed on the workstation | AZ-3739, AZ-3740 | CI skeleton installs pinned versions | clear |

## Commit

`[AZ-3739] [AZ-3740] [AZ-3741] [AZ-3744] [AZ-3763] [AZ-3764] Add helpers, scripts, images, licence` — Loop: 1

## Next Batch: AZ-3761 (CI skeleton) and AZ-3742 (test harness skeleton); then AZ-3765 sc_clock, which needs AZ-3761
