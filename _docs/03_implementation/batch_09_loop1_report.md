# Batch Report

**Batch**: 9
**Tasks**: AZ-3768_contracts_part1, AZ-3967_sync_host_traits_foundation, AZ-3890_daemon_client, AZ-3788_cli_skeleton
**Date**: 2026-11-06

Four parallel workers on disjoint crates (`contracts`, `sync-core`, `desktop/settings`, `server`). Workers edited three files outside their path, each for a named reason: `shared/config/src/table.rs` (AZ-3967, closed key table), `desktop/settings/test-tools/tests/not_shipped.rs` (AZ-3890, allow the dev-dependency), and `Cargo.lock`.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3768_contracts_part1 | Done | `contracts` (registries, identity, admin, quota ports; 19 own traits plus re-exports), 7 test files | 32 integration + 2 compile_fail doctests | AC-2: the 11 `&mut DbTx` methods have doubles that never run |
| AZ-3967_sync_host_traits_foundation | Done | `sync-core` host traits, `SyncConfig`, request path; 12 `sync_*` config keys in `shared/config` | 22 new (sync-core total 66) + `sc_config` tests | Config ranges invented; `bearer()` is synchronous |
| AZ-3890_daemon_client | Done | `sunnycloud-settings` lib: `DaemonClient`, trust checks (unix, windows), backoff, frame cap | 34 + testkit 46 (1 ignored) | Windows SID check and a real second-user squatter NOT RUN |
| AZ-3788_cli_skeleton | Done (AC-9 partial) | `sunnycloud-server` lib + bin: command tree, exit codes, `--json`, guard, secret writer, lazy context, harness | 39 new (server total 47) | Log line lacks `component`, `command`, `exit_code` fields |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 9 findings, none blocking: unbounded `Retry-After` wait, blocking `bearer()` on an async task, extra `Unexpected` variant on every port enum, untested `&mut DbTx` doubles, CLI log fields, CLI settings read from the environment only, duplicated endpoint rules in the settings client, hand-rolled manifest scan, invented sync config ranges.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as `ensure-db-roles.sh` creates them). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, wasm32 builds of `sync-core-wasm`, `sc_ids`, `sc_names`, `sync-core-proto`, `cargo deny check advisories bans licenses sources`; from the root `check-pinning.sh`, `check-no-sql-now.sh`, `check-db-gates.sh`, `check-audit-sql.sh`. All 13 steps exit 0.

## Test Suite

- Total: 830 (workspace, default features, real PostgreSQL 18)
- Passed: 829
- Failed: 0
- Skipped: 1 (AZ-3888 AC-7, `#[ignore]` with its reason)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `async-trait` is the object-safe async mechanism; `contracts` re-exports `async_trait` | AZ-3768 contract Conventions | Owner may veto (alternative: hand-boxed futures like `CheckFuture`) | unclear |
| 2 | Every part-1 async method returns `Result<_, PortError>` and every enum has `Unexpected(Fault)`, beyond the contract tables; `DoctorCheck::run` and `AdminService::health` are infallible | AZ-3768 AC-6, open point 7 | Owner confirms | unclear |
| 3 | Field lists invented minimally for `CycleStats`, `ProviderInfo`, `SessionInfo`, `DeviceInfo`, `InviteInfo`, `AccountInfo`, `AccountFilter`, `Page`, `IdentityView`, `Reserved`, `RevokeReason`, `CallbackParams` | AZ-3768 flagged concerns | Owners of E-15, E-17, E-22 replace them | unclear |
| 4 | `AccountStatus{Active,Disabled,Deleting}` added next to the contract's `AccountState`; `Role` has no `member`; `DevSeed` refusal is `RefusedInProduction`; `InviteService::create_invite` returns the token only | AZ-3768 open points 4, 5 | Owner confirms | unclear |
| 5 | The `&mut DbTx` doubles cannot run without a live `sc_db::Db::run`; a `sqlx` dev-dependency is forbidden | AZ-3768 AC-2 | Component epics exercise them with their databases, or the owner allows a dev-dependency | unclear |
| 6 | New `sync_*` keys in the closed key table with invented ranges; `sync_hash_cpu_share` is a percent Count; `sync_retry_base` is now a defaulted Dur (500ms) | AZ-3967 AC-3 | Owner reviews `table.rs`; helper 01 doc table updated in this batch | unclear |
| 7 | Request path takes a `Transport` trait as its seam; E-23/c07 implements it (TLS 1.3 only, ALPN h2, windows, `X-Sunnycloud-Client`, `Authorization` from `WireRequest.bearer`) | AZ-3967 Excluded | c07 | unclear |
| 8 | Host traits are synchronous; executors call them from blocking tasks | AZ-3967 host_traits.md | Owners of c09/c10/c11 note it | unclear |
| 9 | `UploadCheckpoint.state` has `Hashing`/`Uploading`/`Committing`, so `session_id` and `file_hash` are `Option`; `BatchRecord.entries` reuses `BatchEntry` (superset of the documented fields) | AZ-3967 open points 1, 2 | Owner confirms | unclear |
| 10 | `CheckpointStore` returns `StoreError`; `SyncError` has no Store variant | AZ-3967 | Each operation task chooses; owner decides | unclear |
| 11 | `Retry-After` has no cap; waited in full unless `cancel()` | AZ-3967 AC-5 | Owner decides on a cap | unclear |
| 12 | `contracts` has its own server-side `Credential` enum, so AC-1's "nothing in contracts names them" covers only the five traits | AZ-3967 AC-1 | Rename one if strictness is wanted | unclear |
| 13 | Endpoint path rule, modes, 1 MiB frame cap and the capped line reader are copied from the daemon | AZ-3890 flagged concerns | Move into `ipc_schema` when the contract freezes | unclear |
| 14 | Trust modes mirror the daemon exactly (dirs 0700, socket 0600); 0400 is refused. On Windows an elevated daemon with an unelevated window may be refused as untrusted | AZ-3890 | E-25/c14 checks against the real daemon; revisit on Windows | unclear |
| 15 | Reconnect does not re-subscribe; the refresh-policy owner calls `subscribe()` on each `Connected` | AZ-3890 | E-25/c11 | clear |
| 16 | `not_shipped.rs` changed to allow the testkit as a dev-dependency only | AZ-3890 | As built | clear |
| 17 | CLI log line lacks `component=cli`, `command`, `exit_code`: `sc_observability` has no `Component::Cli` and drops unknown fields | AZ-3788 AC-9 | Add `Component::Cli` and the fields in `sc_observability`, then switch `log.rs` | unclear |
| 18 | CLI loads settings from the environment only; on failure it warns and uses defaults | AZ-3788 | c04 decides config-file loading and config-free commands | unclear |
| 19 | CLI readings of open points 1, 2, 4 and the guard (`cli_confirm_required = off` skips it, a declining TTY gives exit 3, `migrate revert --steps` default 1) | operator_command_surface.md | Recorded as contract v1.1.0 | unclear |
| 20 | `version` commit is read from `option_env!("SUNNYCLOUD_GIT_COMMIT")`, else `unknown`; `server` is now lib + bin | AZ-3788 | c15 adds a `build.rs`; `module-layout` says "Public API: none" | clear |
| 21 | Carried open from batches 7 and 8 (purge cutoff, system scope, mirror line, range table, planner give-up, G-03-1, bandwidth -1, UI driver, `rustls` pin, db duration summary) | earlier reports | Owner | unclear |

## Commit

`[AZ-3768] [AZ-3967] [AZ-3890] [AZ-3788] contracts part 1, sync host traits, daemon client, cli skeleton` — Loop: 1

## Next Batch: AZ-3959 http_sync_client_connect (needs AZ-3967), AZ-3853 decoder runner socket loop, AZ-3868 worker crate skeleton, AZ-3769 contracts part 2 (needs AZ-3768)
