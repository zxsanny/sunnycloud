# Batch Report

**Batch**: 11
**Tasks**: AZ-3682_identity_crate_migration_config, AZ-3792_janitor_scheduler, AZ-3892_platform_fs_linux, AZ-3960_sync_tree_operations
**Date**: 2026-11-06

Four parallel workers on disjoint crates (`components/01_identity` with a new migration, `server`, `desktop/daemon`, `sync-core`). Parent edits: the `sc_db` and `sc_audit` tests that the identity migration broke (12 tests: the embedded-migration list and the `accounts` insert), and `sc_observability` contract notes are unchanged. Shared files touched: `Cargo.lock`, `server/Cargo.toml`, `desktop/daemon/Cargo.toml`, `sync-core/Cargo.toml`, `server/tests/crate_graph.rs` (dev-only `sqlx`).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3682_identity_crate_migration_config | Done | `sunnycloud-identity` config section + error + test harness; migration `20261105110000_create_identity_tables` (up and down) | 39 (11 config, 13 constraints, 9 harness, 2 migration, 4 rls) | Config providers come from the flat key table, not a `providers[]` list; AC-20 limit numbers not invented; `sc_db`/`sc_audit` tests updated by the parent |
| AZ-3792_janitor_scheduler | Done | `server::janitor` (engine, metrics, `scheduler_alive`, audit-retention janitor) | 15 new (7 scheduler, 3 ops, 4 audit, 1 locks) | AC-4 and AC-5 run in-process: the `serve` stack does not exist yet |
| AZ-3892_platform_fs_linux | Done | `sunnycloud-daemon::platform_fs` (trait, `FsError`, fake, Linux impl with `openat2` and fallback, freedesktop trash, `FileSource` adapter) | 30 unit + 21 integration (1 ignored) | ext4 casefold, XFS and a real second disk not run; casefold true branch verified by hand on a casefold tmpfs |
| AZ-3960_sync_tree_operations | Done | `sync-core`: seven tree operations on `SyncApi` / `HttpSyncClient` | 16 new (109 in the crate) | `EditedSince{current}` can only carry `None` (no wire field); request and response bodies are the worker's assumptions |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 10 findings, none blocking: `move_node`/`delete_node` are re-sent after a lost answer (false `NotFound`/`NameConflict`); `EditedSince.current` is always `None`; `ProviderConfig.kind` is a `String` whose validation cannot fire in production; the `public_url` rule duplicates `sc_config`; the per-test database template is now copied three times; no index on `sessions.prev_refresh_hash`; the janitor name is interpolated into the log text; a second concurrent `Scheduler::stop` returns before the loops end; the `decode` warning may echo server data; `mtime_ns` is computed in two places.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as in earlier batches). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo test -p sc_observability` without the `db` feature, the dependency-graph check of the worker and decoder, the four wasm32 builds, `cargo deny check advisories bans licenses sources`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All 15 steps RC 0.

## Test Suite

- Total: 1083 (workspace, default features, real PostgreSQL 18)
- Passed: 1081
- Failed: 0
- Skipped: 2 (AZ-3892 AC-7 casefold true branch, needs `SC_CASEFOLD_DIR`; AZ-3888 AC-7 UI driver)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `EditedSince{current}`: `sc_error::BusinessParams` has no revision id, so the client returns `current: None`; the server knows `current_revision` but the problem body drops it | AZ-3960 AC-3, `sync_api.md` open point 2 | E-02 / E-21 decide the field | unclear |
| 2 | Tree request and response bodies are not in any doc: `POST /nodes {parent_id,name}`, `PATCH /nodes/{id} {parent_id,name}`, `DELETE /nodes/{id}?base_revision=`, `GET /nodes/{id}/children?after=`, `POST /revisions:lookup {content_hashes}` answering a bare array | AZ-3960 | Pin in `api_v1_surface.md` / OpenAPI; only `tree_ops.rs` changes | unclear |
| 3 | `revisions:lookup` with 1,000 hashes is about 70 KB, above the `Small` body class | `api_v1_surface.md` open point 4 | E-21 assigns `Query` | unclear |
| 4 | `create_folder` is sent once; `move_node` and `delete_node` use the shared retry (see review finding 1) | AZ-3960 | Owner decides whether a lost answer to a move or delete is retried | unclear |
| 5 | The identity migration makes `accounts.display_name` and `role` `NOT NULL` without defaults, as data model 4.1 says; every `accounts` insert (tests included) must supply them | AZ-3682 | As built; the 12 broken tests were fixed | clear |
| 6 | Identity config reads the flat provider keys (`github_client_id`, `oidc_*`, `allowlist_emails`) of the key table; a `providers[]` list needs an owner decision; the spec's "rule lives once" for `public_url` conflicts with `ApiSettings::validate` | AZ-3682, 01 §6 | Owner decides the provider format | unclear |
| 7 | AC-20 limit numbers: `device_grant_ip_limit` and `invalid_token_ip_limit` are optional with no default number in the table; passed through as `Option<u64>` | AZ-3682, AZ-3691 | E-15 owner supplies the numbers | unclear |
| 8 | Docs disagree: contracts `ClientKind::Desktop` vs data model CHECK (`web`, `android`, `ios`); 01 text says `member`, data model says `user`; no index on `sessions.prev_refresh_hash`; a pending `device_grants` row has no account so inserting it needs system scope | AZ-3682 | Data model and 01 aligned by the E-15 owner | unclear |
| 9 | Janitor label: `task` (observability.md) not `name` (IT-12-7); `scheduler_alive` is Failing after `stop` or when a loop ended, never because a janitor keeps failing; `stop` waits for a running cycle; the first cycle runs one interval after start; a panic counts as `failed` | AZ-3792 flagged concerns | As built; fix IT-12-7 | unclear |
| 10 | The log schema has no task or janitor field, so the janitor name is in the message text | sc_observability | Add a `task` field in v1.4.0 | unclear |
| 11 | The audit-retention janitor needs an interval; no `audit_retention_interval` key exists | AZ-3792, E-22/c03 | E-22/c03 reuses a key or adds one | unclear |
| 12 | `server/tests/crate_graph.rs` now allows `sqlx` as a dev-only dependency of the server (test databases); a normal dependency is still refused | AZ-3792 | As built | clear |
| 13 | `PlatformFs` additions: `open_dir`, `discard_temp`, `EntryKind::Other`; every `name` is a relative path beneath a directory handle; `walk` is non-recursive with an `unrepresentable` count; `Locked` is Windows-only; only `.Trash-<uid>` is supported; the fake is `#[cfg(test)]` (in-crate use only); the trait is not object-safe | AZ-3892 | Update the spec and 14 description §2 | unclear |
| 14 | Linux trash: the home trash is used only when the file is on the data home's volume, otherwise `<mount>/.Trash-<uid>`; `EXDEV` becomes `TrashUnavailable`; a btrfs subvolume is treated as a mount point; trash tests need `/dev/shm` as a second mount and fail on a stale `/dev/shm/.Trash-<uid>` | AZ-3892 | As built | unclear |
| 15 | Carried from earlier batches and still open: Retry-After cap, `bearer()` blocking, `Unexpected` variant (part 1), `&mut DbTx` doubles, CLI log fields, UI driver, db duration summary, invented sync ranges, image checks never run | batches 2-10 | See their reports | unclear |

## Commit

`[AZ-3682] [AZ-3792] [AZ-3892] [AZ-3960] identity schema and config, janitor scheduler, PlatformFs Linux, sync tree operations` — Loop: 1

## Next Batch

Ready now: AZ-3749 quota skeleton, AZ-3816 admin crate skeleton, AZ-3963 small upload batch, AZ-3964 verified download, AZ-3965 feed client, AZ-3966 token lifecycle (all `sync-core`, serialize or one worker), AZ-3683 authenticator and scopes cache, AZ-3684 provider sign-in callbacks, AZ-3891 local index store, AZ-3790 per-command config.
