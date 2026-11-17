# Batch Report

**Batch**: 84
**Tasks**: AZ-4022_identity_doctor_checks, AZ-3809_doctor_root_checks
**Date**: 2026-11-17

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4022 identity doctor checks | Done | 01 `src/doctor.rs` (new), `provider/{mod,github,oidc}.rs` (`probe`, `build_all`), `sign_in/{mod,rules}.rs`, `invite_repo.rs`, `auth_repo.rs`, `lib.rs`, `Cargo.toml`; `tests/doctor.rs` (new); `tests/support/provider.rs` | See Test Suite | None open |
| AZ-3809 doctor root checks | Done | `sc_db` (`inspect.rs` new, `db.rs`, `roles.rs`, tests); contracts (`BlobRevision`, `TreeReader::describe_blob_revisions`); 02 (`revision_repo.rs`, `reader_revisions.rs`); 04 (check removed); server (`database_check.rs`, `missing_blobs_check.rs`, `quota_drift_check.rs` new; `doctor_command.rs`, `root/lists.rs`); test doubles of 07, 08, 11 | See Test Suite | Reserved-counter drift not reported (Low) |

Owner decisions of 2026-11-17 (asked in this session): an unreachable provider is `Warn`; setup pending is `Ok` with the message and the `sunnycloud admin setup-token` remedy; clock skew above 2 s and quota drift are both `Warn`; the missing-blob report gets account and revision id through a new additive `TreeReader::describe_blob_revisions`.

**AZ-4022.** `IdentityProvidersCheck` (tag `pre-flight`, holds no database) prints per provider the redirect URI from `PublicUrl::callback_url`, the one `begin` uses, and probes the provider through the same outbound client under `provider_timeout`: OIDC discovery, or a GET of the GitHub token endpoint (any answer short of a 5xx counts). `IdentitySetupCheck` reads "an account exists" and the open setup token's creation time in one system-scope read and reports setup done, pending with the token age, or pending with the remedy. `provider::build_all` is now shared by sign-in and the check, so the clients are built in one place.

**AZ-3809.** `Db::inspect` (new in `sc_db`) connects as the API role without refusing a wrongly privileged one and reports server version, the role problem, the schema state and the server clock, so `DatabaseCheck` can name a `BYPASSRLS` or schema-owner role, pending or dirty migrations and clock skew instead of `doctor` failing to start. Because `Db::connect` refuses such a role, `doctor` now treats two build failures as findings: a configuration that does not load becomes one `Fail` per rejected key (named, never the value), and a stack that does not build runs the database check and adds one `stack` finding. `MissingBlobsReport` replaces 04's ids-only `MissingBlobsCheck` (removed with its three tests): one `Fail` per revision with path, account and revision id, one per missing blob no revision holds. `QuotaDriftCheck` calls `QuotaAdmin::recompute(.., repair = false)` for every account except those being deleted. `DoctorChecks` order: database, key, volume, retention, missing blobs, quota drift, media worker, providers, setup.

## Code Review Verdict: PASS

No findings. Follow-ups recorded: `describe_blobs` now has no production caller (kept: contract unchanged); `QuotaAdmin::recompute` returns the `used` counter only, so a drifted `reserved` counter is not reported; the public URL / TLS / HTTP/2 / proxy-body probe check of 12 §6 still has no task.

## Test Suite

- Crates whose code or test doubles changed, `--no-fail-fast`: `sc_db` 56, `sunnycloud-identity` 268, `sunnycloud-server` 220 (0 failed each); `contracts`, `sunnycloud-blobstore`, `sunnycloud-admin`, `media-api`, `sunnycloud-http-api`, `sunnycloud-tree`: 1301 passed, 8 failed, all in `sunnycloud-admin` `purger_logs::resume` ("the account is not gone after 400 cycles") while the host load average was about 24 (other users' jobs share this machine); the same ten tests pass alone (14 s). 07 changed only by one test-double method.
- New tests: `sc_db` 5 (`Db::inspect`), 01 5 integration + 4 unit, 02 1 (`describe_blob_revisions`), contracts 1 assertion, server unit 8 and 5 binary tests (`doctor_findings`: configuration findings without a secret, schema-owner role plus stack finding, healthy stack, quota drift naming one account and changing no row, three revisions with missing blobs listed with path, account and revision id).
- Not covered end to end: AC-3 of AZ-3809 with a database clock offset of 3 s (the threshold and the clock read are tested separately), AC-2 of AZ-4022 with a real mock-idp (the in-process provider and a refused address stand in; mock-idp does not exist yet).
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.

## Commit

`[AZ-3809] [AZ-4022] Doctor database, config, blob and quota checks, identity checks` - Loop: 1
