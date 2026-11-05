# Batch Report

**Batch**: 6
**Tasks**: AZ-3771_media_shared_skeleton, AZ-3783_sc_db_migrator_accounts_base, AZ-3955_sync_core_proto, AZ-3914_ipc_server_endpoint
**Date**: 2026-11-05

Four parallel workers on disjoint paths. The parent also fixed a root cause that two workers hit: `sc_clock` pulled tokio into every crate that used it, so `sc_names` and `sync-core-proto` (wasm32 client crates) could not stay runtime-free. `sc_clock` now has a default `runtime` feature (clock trait, system/manual/offset clocks, shutdown token), and `sc_ids` has a default `new-id` feature; `sc_names`, `sync-core-proto` and `media-shared` use them with `default-features = false`.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3771_media_shared_skeleton | Done | crate (4 src, 6 test files) | 42 tests | Many contract-silent shapes chosen (see below) |
| AZ-3783_sc_db_migrator_accounts_base | Done | sc_db (migrate module, stats), first migration pair, `ensure-db-roles.sh`, README | 49 tests + 2 doctests on real PG 18; check-migrations run with a real migration | Statement timeout also applies to migrations |
| AZ-3955_sync_core_proto | Done | new crate (12 src, 7 test files), crate_graph list | 42 tests, wasm32 build; AC-1 now green after the sc_clock/sc_ids feature split | Wire shapes of outcome enums chosen by the worker |
| AZ-3914_ipc_server_endpoint | Linux done; AC-2 not run | daemon lib + `ipc` module (unix, windows, server, connection, lines) | 26 tests (Linux); Windows part compile- and clippy-checked only | AC-2 NOT RUN; AC-1 cross-user denial unproven |

## Code Review Verdict: PASS_WITH_WARNINGS

Focused review of the joined tree: 5 findings, all skipped with reasons (migration statement timeout, Windows pipe unrun, cross-user denial unproven, contract-silent shapes in media-shared, outcome wire tags in sync-core-proto). The workspace silent-discard gate also caught one `let _:` in a new test; fixed.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, wasm32 builds of `sync-core-wasm`, `sc_ids`, `sc_names`, `sync-core-proto`, `cargo deny check advisories bans licenses sources`; from the root `check-pinning.sh`, `check-no-sql-now.sh`, `check-db-gates.sh`. The migration scripts ran against a real migration in the worker's environment (no `psql` here; a shim was used); CI uses the real `psql` and `ensure-db-roles.sh`.

## Test Suite

- Total: 442 (workspace, default features, real PostgreSQL 18)
- Passed: 442
- Failed: 0
- Skipped: 0

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The Migrator pool inherits `db_statement_timeout` (30 s); a long migration or a wait for the advisory lock is cancelled | AZ-3783 | Give the Migrator role no (or its own) statement timeout | unclear |
| 2 | `status` and `check_version` return `Result`; `SchemaState`/errors gained `Dirty`, `WrongRole`, `Db`, `ChecksumMismatch`; `MigrationReport.reverted`, `checksum_mismatch` added | AZ-3783 | Update `contracts/shared_db/sc_db_migrations.md` | clear |
| 3 | `ON DELETE RESTRICT` raises SQLSTATE 23001, mapped to `ForeignKeyViolation` | AZ-3783 AC-2 | Contract and AC text one-line fix | clear |
| 4 | sqlx 0.9: an interrupted `-- no-transaction` migration leaves no bookkeeping row and re-runs whole; failed migration can leak the advisory lock on a pooled connection (the migrator uses one connection and closes it on failure) | AZ-3783 | Migration authoring rule in the README: idempotent no-transaction files | clear |
| 5 | In PostgreSQL 18, NOT NULL columns appear as `pg_constraint` rows; later constraint-count tests must filter by `contype` | AZ-3783 | Note for migration owners | clear |
| 6 | CI roles come from `scripts/ci/ensure-db-roles.sh` (no passwords); the compose database still uses the init script | AZ-3783 | Keep in sync with `10-roles.sh` | clear |
| 7 | media-shared shapes the contract leaves open: `OutputSpec`, `Params` preset enum, `AudioPlan`, `LimitKind`, `Limits.wall` in ms, `content_hash` as 64 lowercase hex, `build_timestamp` seconds | AZ-3771 | E-18, E-19, E-20 owners confirm; no route-body wrapper types or message envelope defined | unclear |
| 8 | sync-core-proto wire tags (`outcome`, `status`), `Published`/`ConflictCopy` fields, `expires_at` as String, `client_meta` as JSON value, `SessionTokens.refresh` optional | AZ-3955 | Check against the OpenAPI document (E-21) | unclear |
| 9 | `ServerUrl` and lane rules (open points 3 and 5) are recorded only as rustdoc | AZ-3955 | Copy into `sync_core_proto.md` when the contract is frozen | clear |
| 10 | IPC: max line length 1 MiB, bounded 256-line outbound queue, 10 s write stall closes the connection, leftover-socket rule, proposed endpoint names (`$XDG_RUNTIME_DIR/sunnycloud/daemon.sock`, `\\.\pipe\sunnycloud-daemon-<SID>`) | AZ-3914 | Add to `ipc_schema.md`; owner confirms | unclear |
| 11 | Windows pipe (AC-2) is compile-checked only; second-user denial on Linux (AC-1) unproven | AZ-3914 | Run on the Windows VM and a multi-user lab host | unclear |
| 12 | Daemon logs with `tracing`; `interprocess` 2.4.7 named in the spec is not used (tokio's own unix and windows pipe types instead) | AZ-3914 | Confirm logging choice | unclear |
| 13 | `sc_clock` `runtime` feature split and `sc_ids` `new-id` feature | AZ-3765, AZ-3766 | Contracts mention the features | clear |
| 14 | `sc_ids` AC `getrandom` `wasm_js` pulls wasm-bindgen transitively into the sc_ids wasm tree | AZ-3766 | Accepted: no direct dependency | clear |

## Commit

`[AZ-3771] [AZ-3783] [AZ-3955] [AZ-3914] media-shared, migrator, proto, IPC` — Loop: 1

## Next Batch: AZ-3784 sc_audit (needs 3783), AZ-3888 stub daemon UI driver, then AZ-3786 sc_observability, AZ-3768 contracts part 1
