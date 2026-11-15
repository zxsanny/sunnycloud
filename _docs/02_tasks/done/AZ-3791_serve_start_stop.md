# `serve` start and stop sequence

**Task**: AZ-3791_serve_start_stop
**Name**: `serve` start and stop sequence
**Description**: Start the server in the documented order, refusing before any listener on an unsafe role, schema, key or epoch state, and stop it in the documented order on SIGTERM (exit 0) or SIGINT (exit 130)
**Complexity**: 5 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3790_per_command_config, AZ-3792_janitor_scheduler, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3725_master_keyring, AZ-3727_group_commit_barrier, AZ-3779_epoch_admin, AZ-3756_transfer_janitors_reservations, AZ-3801_crate_skeleton_accept_loop, AZ-3833_graceful_shutdown, AZ-3786_sc_observability, AZ-3742_test_harness_skeleton
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3791
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — `serve` row and exit codes (E-22/c02).
- `_docs/02_document/contracts/shared_db/sc_db_access.md`, `shared_db/sc_db_migrations.md` — role verification, `check_version`.
- `_docs/02_document/contracts/04_blobstore/epoch-marker-and-health.md` — marker semantics.

## Problem

Restore and key safety depend on order (R02, AC-72). If a listener opens before the key check, a wrong master key serves errors or worse; if it opens before the epoch-marker check, devices ahead of a restored database keep a stale cursor. The API must never migrate itself (S4-2). On shutdown, the order decides whether upload progress and the last group sync reach disk before the pool closes.

## Outcome

- Start order: bind and validate configuration (E-22/c04) → connect with `db_api_url`, role verified by helper 07 → `Migrator::check_version` must be `Current` → master key check (`MasterKeyring::load_and_verify`) → epoch marker check (`EpochAdmin::marker_status`: refuse when the database epoch is below the marker) → construct components (E-22/c03) → build `HttpServer` (listeners `:8080`, `:8081`, `:9090`) → register the late lists → start the scheduler → ready.
- Every refusal exits non-zero before any port accepts a connection, with a message that names the fix.
- Shutdown on SIGTERM within `shutdown_grace` (25 s): readiness 503 → `HttpServer` stops accepting, wakes held long-polls and drains (11) → stop the scheduler → flush upload progress checkpoints (03) → final group sync (04) → close the pool; exit 0. SIGINT runs the same order and exits 130.
- One INFO log line per start-up and shutdown step, in order.

Narrowed against the plan: the end-to-end refusals for a pending migration and a missing provider are AZ-3654 (E-22/p01) AC-3; the refusal on an epoch below the marker and on a wrong master key are AZ-3666 (E-22/p13) AC-1 and AC-72. This task owns the sequence and the remaining refusal cases.

## Scope

### Included
- The start-up order and the refusal for each failed step, with its message.
- The shutdown order, the grace period and the SIGTERM and SIGINT exit codes.
- Start-up and shutdown log lines.

### Excluded
- What each check decides (helper 07, 04, 05), connection draining inside 11 (E-21/c17), the setup-token callsite at start (E-22/p01), the scheduler engine (E-22/c06), construction (E-22/c03).

## Acceptance Criteria

**AC-1: Start order (IT-12-7 order part)**
Given a valid install and `log = json`
When `serve` starts
Then the step log lines appear in the order configuration, database role, schema version, key check, epoch marker, services, listeners, late lists, scheduler, ready, with increasing timestamps, and `/readyz` returns 200 within 10 s (PT-12-1)

**AC-2: Newer database refuses; `serve` never migrates (FT-N-39, NFT-RES-14 step 1)**
Given a database whose schema is newer than the binary (`UnknownNewer`), and separately one with a pending migration
When `serve` starts
Then it exits non-zero within 10 s, no port of 8080, 8081 or 9090 accepts a connection (probe every 100 ms), the message names `sunnycloud migrate`, and the migrations table is unchanged

**AC-3: Key refusals (AC-72, IT-12-6 e, f)**
Given, in turn, a missing master key file, a 31-byte key file, and an absent `.keycheck` on a volume that holds blobs
When `serve` starts
Then each exits non-zero before any listener, the message names the key problem (`FileMissing`, `WrongLength`, `KeyCheckAbsentButBlobsExist`), and no key byte appears in any output

**AC-4: Role refusal (IT-12-6 j)**
Given `db_api_url` pointing at the schema-owner role
When `serve` starts
Then it exits non-zero before any listener and the message names the database role check

**AC-5: Graceful shutdown order (IT-12-8)**
Given `serve` with 20 held long-polls, 10 uploads in progress and `shutdown_grace = 3s`
When SIGTERM is sent
Then the log shows readiness 503, listeners stop accepting, long-polls woken, connections drained, scheduler stopped, upload checkpoints flushed, final group sync, pool closed, in that order; the process exits 0 within grace plus 2 s; after a restart clients resend only unacknowledged blocks (NFT-RES-22)

**AC-6: SIGINT exits 130 (G-12-7 default)**
Given a running `serve`
When SIGINT is sent
Then the shutdown order of AC-5 runs and the exit code is 130

## Non-Functional Requirements

**Performance**
- Start to `/readyz` 200: target 10 s, failure above 30 s (PT-12-1).
- Each refusal within 10 s.

**Reliability**
- Coverage of the start-up order and refusal mapping at least 90% (quality thresholds).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Step sequence with recording steps | Listeners only after key and epoch steps |
| AC-2..AC-4 | Each failed step | Non-zero exit, no later step runs, message names the fix |
| AC-5 | Shutdown sequence with recording steps | Documented order; pool last |
| AC-6 | SIGINT vs SIGTERM | 130 vs 0 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Valid stack, `log = json` | Start (IT-12-7) | Ordered steps; ready within 10 s | PT-12-1 |
| AC-2 | Newer schema; pending migration | Start (IT-12-6 a, b) | Non-zero, no port, names `sunnycloud migrate`, table unchanged | FT-N-39, NFT-RES-14 |
| AC-3 | Missing key, 31-byte key, absent `.keycheck` with blobs | Start (IT-12-6 e, f) | Non-zero before listeners; no key bytes | AC-72 |
| AC-4 | Schema-owner role URL | Start (IT-12-6 j) | Non-zero, names the role check | NFT-SEC-46 |
| AC-5 | 20 long-polls, 10 uploads, grace 3 s | SIGTERM (IT-12-8) | Ordered shutdown; exit 0 within 5 s | NFT-RES-22 |
| AC-6 | Running `serve` | SIGINT | Same order; exit 130 | |

## Runtime Completeness

- Named capability: the documented shutdown order, including "flush upload progress checkpoints" (03) and "final group sync" (04).
- Production code that must exist: calls to the real 03 and 04 operations for both steps, in that order, before the pool closes.
- Allowed external stubs: none in the blackbox tests.
- Unacceptable substitutes: a log line without the operation, skipping a step because its port is not named yet, or relying on the next start to recover.

## Constraints

- `serve` checks the schema; it never runs a migration. `db_migrator_url` is not used.
- Tests run the real binary against PostgreSQL 18 and a real blob volume; refusal conditions are arranged with harness access, never inside the binary.

## Risks & Mitigation

**Risk 1: A refactor binds a listener early**
- *Risk*: building `HttpServer` before the checks opens a port.
- *Mitigation*: AC-2..AC-4 probe all three ports from process start.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No `contracts` port is named for "flush upload progress checkpoints" (03 offers only the idle-flush janitor) or for the "final group sync" (04) at shutdown | E-14, E-10 owners | open | Medium |
| A `.epoch` marker absent on a volume that holds blobs fails 05's doctor check, but `serve`'s refusal rule names only "database epoch below the marker" | E-12 / E-22 owner | open | Low |
| The exact non-zero code (2 or 3) per refusal kind is not stated; IT-12-6 accepts either (contract open point 5) | E-22 owner | open | Low |
| `deployment_procedures.md` says SIGTERM gets 120 s, while helper 01 and 11 set `shutdown_grace` to 25 s; this task follows the config default | Deployment docs owner | open | Low |

## Owner decisions (2026-11-15)

- `HttpConfig` keys (from AZ-3789): new `sc_config` keys `public_addr` (default `0.0.0.0:8080`), `session_cookie` (default `__Host-session`, the name the OpenAPI document publishes) and `binding_cookie` (default `__Host-signin-binding`); `serve` maps `ServeSettings` to `HttpConfig` and passes `root::api_services`.
- Shutdown steps with no port (flagged concern, Medium): add two small operations inside this task. 03: save every upload's unsaved progress now (the idle-flush work for every entry, regardless of idle time). 04: wait until every open sync group has finished. `serve` calls both, in that order, before the pool closes.

## Implementation status (2026-11-15)

| AC | Status | Evidence |
|----|--------|----------|
| AC-1 | Done | `src/server/tests/serve_start.rs::start_steps_are_logged_in_order_and_readyz_is_200_within_10s` (real binary, `log = json`, 10 step lines in order, non-decreasing `ts`, `/readyz` 200 < 10 s) |
| AC-2 | Done | `serve_start.rs::a_database_newer_than_the_binary_refuses_and_names_migrate`, `::a_pending_migration_refuses_names_migrate_and_serve_does_not_migrate` (exit 2 < 10 s, the three listeners probed every 100 ms never accept, `_sqlx_migrations` unchanged) |
| AC-3 | Done | `serve_start.rs::a_missing_master_key_file_refuses_with_file_missing`, `::a_31_byte_master_key_refuses_with_wrong_length_and_prints_no_key_byte`, `::a_volume_with_blobs_and_no_keycheck_refuses` |
| AC-4 | Done | `serve_start.rs::the_schema_owner_url_refuses_naming_the_role_check` |
| AC-5 | Done | `serve_stop.rs::sigterm_runs_the_shutdown_in_order_and_a_restart_resumes_only_the_unacknowledged_blocks` (20 held long-polls answered 200 after the signal, 10 uploads, grace 3 s, exit 0 < 5 s, after a restart `need_blocks` lists only the unacknowledged blocks), `::the_pool_closes_last` (`serve stop: final group sync` then `serve stop: pool closed`). `sc_db::Db::close` was added to helper 07 inside this task (pool close is the last shutdown step) |
| AC-6 | Done | `serve_stop.rs::sigint_runs_the_same_shutdown_order_and_exits_130` |

As built: `src/server/src/serve/` (start, stop, `HttpConfig` mapping); 11 logs its four shutdown steps
(`BoundServer::serve`); 03 `PgUploadService::flush_all` shares the idle flush's code path; 04
`SyncBarrier::sync_open_groups` is the final group sync; the scheduler is built with the readiness list and started
after the late lists (`root::start_scheduler`); the keyring, `.epoch` marker and `PgEpochAdmin` have their own
builders so the start checks construct no other component; new keys `public_addr`, `session_cookie`,
`binding_cookie`. Unit tests with recording steps were not added: the order and the refusal mapping are checked on
the real binary from its log, plus unit tests of the refusal texts (`serve/start.rs`) and of the 0/130 mapping
(`serve/mod.rs`).

Open: `update_url` has no key; with no `min_client_version`
`server-info` advertises `0.0.0`; spawned tasks (request handlers, janitor loops) log through no subscriber because
the CLI installs its subscriber per future, not globally; the SPA bundle is not wired to `HttpServer::with_spa`.
