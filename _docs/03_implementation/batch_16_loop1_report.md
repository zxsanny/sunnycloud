# Batch Report

**Batch**: 16
**Tasks**: AZ-3689_mobile_code_identity_janitor, AZ-3690_account_admin_and_guard, AZ-3871_worker_decoder_client
**Date**: 2026-11-06

Two workers on disjoint crates (`components/01_identity` with AZ-3689 then AZ-3690, `worker` with AZ-3871). Parent edits: the media-library needles of `worker/tests/crate_graph.rs`. The worker's new client serializes the decoder protocol's capabilities message, which has a field named `libheif`, so the bare word matched; the needles are now the library files (`libvips.so`, `libheif.so`, `libavcodec.so`, `libavformat.so`) and one API function of each (`vips_init`, `heif_context_alloc`, `avcodec_open2`, `av_read_frame`), which still detect a dynamic or static link. Owner decision in this batch: delete the regenerable build cache `src/target/debug/incremental` because the disk was full (about 46 GB freed). No allowlist, advisory or schema change.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3689_mobile_code_identity_janitor | Done | `sunnycloud-identity`: mobile branch of `complete` stores a one-time login code (hash only, PKCE challenge, TTL), `exchange_mobile_code` (delete-returning, S256 check in constant time, active-account check, session mint), `IdentityMaintenance` (`Janitor`, batched deletes of expired states, codes, grants, sessions) | 14 new (9 mobile code, 5 janitor) | Code consumed also on a wrong verifier; per-table counts only through `Swept`; grants deleted 1 h after expiry only (no consumption column) |
| AZ-3690_account_admin_and_guard | Done | `IdentityAccounts` implementing `AccountAdmin` and `AccountGuard` (list, lock active admins, set role and state with credential revocation, begin and finalize deletion, shared-lock `ensure_active`) | 14 new (1 unit, 13 integration) | `AccountAdminError` has only `Unexpected`; the caller's transaction must be in the target's scope or system scope |
| AZ-3871_worker_decoder_client | Done | `sunnycloud-worker::decoder_client`: `DecoderConn` over the shared codec (hostile replies are typed violations that drop the connection), `Slot` with backoff, `hello` and `self_check` gate, cached capabilities, readiness check, metrics | 24 new (gate 9, calls 7, hostile 7, memory 1) | Backoff and forbidden targets are constructor arguments; metrics carry a `slot` label; the restart counter can overcount |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 5 findings, none blocking: valid decoder events can keep a call open forever; the reconnect bounds and forbidden targets are not config; the identity janitor drains without a time budget; the account operations silently revoke nothing under the wrong caller scope; `AccountAdminError` is untyped.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as in earlier batches). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo test -p sc_observability` without the `db` feature, the dependency-graph check of the worker and decoder, the four wasm32 builds, `cargo deny check advisories bans licenses sources`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All 15 steps RC 0.

## Test Suite

- Total: 1525 (workspace, default features, real PostgreSQL 18)
- Passed: 1522
- Failed: 0
- Skipped: 3 (AZ-3892 AC-7 casefold true branch; AZ-3888 AC-7 UI driver; AZ-3961 5 GiB single-buffer test)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Account admin: `AccountAdminError` needs typed `AccountNotFound` and `InvalidState` variants in `contracts`; the contract must state that 07's transaction is in the target's account scope or system scope; `begin_deletion` is idempotent and keeps the first request time; `finalize_deletion` needs `deleting` and is a no-op for a missing account; `set_state` on a `deleting` account is refused; "revoke grants" means approved grants become `denied`; listing is ordered by creation | AZ-3690 | Update `account_admin_and_guard.md` and add the variants | unclear |
| 2 | Mobile code: the minted session also requires an active account (`InvalidCode` otherwise); a wrong or expired attempt burns the code (a code interceptor can deny the app its sign-in but not use it); janitor constants (batch 1000, grants 1 h, sessions 30 d) are not config; the janitor has no time budget | AZ-3689 | Update the 01 description; decide on a budget | unclear |
| 3 | Decoder client: no config keys for reconnect backoff (suggested `worker_decoder_reconnect_initial`, `worker_decoder_reconnect_max`) and for the forbidden targets; `worker_selfcheck_ok` and the restart counter carry a `slot` label (`observability.md` lists them unlabelled); `worker_decoder_build_timestamp_seconds` not implemented; an empty `self_check` report fails the gate; `/readyz` is 503 while connecting; the restart counter counts any reconnect after a handed-out connection | AZ-3871 | Add the keys; update `observability.md` | unclear |
| 4 | A valid `Alive` or `Progress` flood from a hostile decoder keeps a call open until the caller's own timeout (c06/c07) | AZ-3871 | Bound events per call in the client or in c07 | unclear |
| 5 | Disk: `src/target` had grown to 133 GB and another project's database started writing at the same time; the disk reached 100% and the owner approved deleting `target/debug/incremental`; the disk was at 94% after the gate | process | Keep `target` below a budget (for example a periodic clean of `incremental`) | unclear |
| 6 | Carried from earlier batches and still open: tree and batch body shapes, `EditedSince.current`, provider list format, audit-retention interval key, `task` log field, `SessionInfo` fields, sliding refresh expiry, image checks never run | batches 2-15 | See their reports | unclear |

## Commit

`[AZ-3689] [AZ-3690] [AZ-3871] mobile login code and identity janitor, account administration and guard, worker decoder client` — Loop: 1

## Next Batch

Ready now: AZ-3749 quota skeleton and AZ-3723 blobstore skeleton (both add a migration and need the per-test database harness: extract a shared test-support crate first), AZ-3893 platform fs windows (cannot be compiled here); the ready list is short until the component crates are unblocked by the skeletons.

## Follow-up: shared test-database crate (owner decision, no ticket)

The per-test database harness (cluster and role setup, migrated template, per-test clone, connection-leak count) existed three times and the next crates (quota, blobstore, tree, ...) would have copied it again. It is now one dev-only crate, `src/shared/testdb` (package `sc_testdb`): `TestDb` with `clone_template`, `clone_empty`, `config`, `api`, `migrator`, `conn_as`, `admin`, `admin_exec`, `admin_count`, `open_connections`, `drop_db` and `finish` (the leak count), plus `START` and `clock_at_start`. The template name carries a hash of the migration files, stale templates and test databases are dropped, and the password and setup lock equal those of `sc_db`'s own tests, so all suites can run in parallel against one server.

Migrated: `sc_audit` tests (support file 279 to 39 lines), identity tests (397 to 112 lines; `Harness`, fixtures and settings stay), and the two server janitor tests (their `#[path]` include of another crate's test file is gone; they use `sc_testdb` and the real `sc_audit` writer). Not migrated: `sc_db`'s own lib tests, which cannot depend on a crate that depends on `sc_db` (its `support.rs` stays at 295 lines; the constants are cross-referenced in the crate docs). Every test binary of the migrated suites has the same pass count as before; `crate_graph` went from 8 to 10 tests.

Guards: `server/tests/crate_graph.rs` now rejects `sc_testdb` as a normal or build dependency of any workspace crate (a passing and a negative test). Owner decision: `scripts/ci/check-db-gates.sh` exempts `src/shared/testdb/` from the sql-text step only (not from the scope-names and run-system steps), through a new helper `in_testdb`.

Gate: all 15 steps RC 0; 1529 passed, 0 failed, 3 ignored. Doc follow-ups: mention `shared/testdb` in the module map and in the 01 `tests.md` Test Data Management section; note the dev-only rule in the dependency rules; the header of `check-db-gates.sh` already names the exemption. Disk after the gate: 96%.

Commit: `[no-ticket] shared dev-only test-database crate sc_testdb` — Loop: 1
