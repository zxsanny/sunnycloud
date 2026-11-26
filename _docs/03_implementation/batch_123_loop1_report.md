# Batch 123, loop 1: AZ-3813 AC-5, lazy construction (AZ-3813 complete)

**Batch**: 123
**Tasks**: AZ-3813_cli_integration_suite (remaining AC-5)
**Date**: 2026-11-26

## What was built

- `command_config.rs`: `ConfigCheck::Services(&[KeyGroup])`. `key-check`, `admin keycheck-init` and `admin rewrap` bind `environment`, the API-role database keys and 04's blobstore keys. `admin setup-token|create|reset|invite` bind `environment`, the same database keys and 01's identity keys. The owners' start validation runs, problems of unbound keys are dropped by the loader, and 01's "a provider is configured" rule (`providers`, no table key) binds `serve` alone. The narrowed settings go into the context's `ServeSettings` slot before any builder runs (`ServiceContext::provide`, new `CliError::AlreadyBuilt`).
- `root/domain.rs`: `IdentityInvites` has its own builder. `OperatorIdentityOps`, `InviteService` and `Identity` share that one instance, so the invite commands build no sign-in service (which refuses without a provider).
- Contract `operator_command_surface.md` 1.4.0: the per-command key lists; open point 9 is resolved for these commands.
- Tests: `each_key_group_lists_exactly_the_keys_its_owner_reads` compares each list with the `settings.<key>` reads in the owner's `config.rs` (the migrator secret is excluded for the API role). There are also unit tests of the narrowed load, `provide` tests, a composition-root test (the invite ports build neither `Identity` nor the limiter), and the updated command-to-check table. The two `#[ignore]`d IT-12-18 tests in `cli_suite.rs` now run and pass. The key-check case had set `master_key_file` to the key text instead of a path, and had no blob folder; both are fixed in the arrangement. `it12_coverage_map.md` IT-12-18 rows now read `pass`.
- Test harness `real_handler()` registers configuration sources and the settings slot, as the real wiring does. `key_commands.rs` now supplies its settings as sources.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3813_cli_integration_suite | Done (AC-1 to AC-7) | 14 files | focused 98/98; full server suite 341/346 | 5 `pipeline_db_restore` need `pg_dump` (not on this host; CI installs it) |

## Code Review Verdict: PASS_WITH_WARNINGS

Review at the medium level reported 5 findings. Fixed: the real-handler harness had no configuration sources, which broke 13 tests (important); the unit tests left temp folders behind (nit). Not fixed:
- The narrowed load still runs other owners' cross-field rules on default values. A rule that names a bound key could refuse a narrowed command (nit).
- The key-group guard scans each owner's `config.rs` only. Today no builder of these commands reads a key outside `from_settings`, except `master_key_file`, which is in the blobstore group (nit).
- The `expect` on `Arc::into_inner` stays: `load_keys` always returns a fresh `Arc`.

## Test Suite

- Full `cargo test -p sunnycloud-server`: 346 tests, 341 passed, 5 failed, 2 ignored (the five `pipeline_db_restore` tests stop at "pg_dump cannot start"; the same five failed in batch 122 for the same reason).
- Rerun after the harness fix: identity_commands, key_commands, rewrap_sigint, scrub_sigint, cli_suite, composition_root, lazy_context, command_config and the lib unit tests, 98 of 98.
- CI-parity: PASS (`cargo fmt --all --check`; `cargo clippy -p sunnycloud-server --all-targets --locked -- -D warnings`, also with `--features vault` and `--features test-clock`; `scripts/ci/check-it-suite.sh` for IT-12: 19 cases PASS within budget, IT-12-18 6.3 s of 40 s).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `admin create`, `reset` and `invite` used to need a sign-in provider too (they built the whole identity service); they now need only the database and identity keys | AZ-3813 AC-5 names setup-token and key-check only | Same narrowing as setup-token: they call only 01's invites | clear (contract: "each command task binds its own keys") |
| 2 | Other commands that build services (`doctor`, `admin user ...`, `scrub`, `admin epoch-bump`, ...) still bind every key | Contract open point 9 | Unchanged; each can get a key list when its task needs one | clear |

## Commit

`[AZ-3813] Lazy config: keyring and invite commands bind only their keys`. Body: one line + `Loop: 1`.

## Next Batch

AZ-3815 (CLI acceptance, performance and security on the compose stack), then AZ-3835 AC-1 and AC-4, then the media and worker suites (AZ-3847, AZ-3878).
