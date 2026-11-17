# Batch Report

**Batch**: 79
**Tasks**: AZ-3797_key_commands
**Date**: 2026-11-17

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3797_key_commands | Done (AC-1 to AC-6, command side) | `key_commands.rs` (new), `dispatch.rs`, `lib.rs`, `commands/admin.rs`, `root/storage.rs`, `serve/start.rs`, `tests/key_commands.rs` (8), `tests/rewrap_sigint.rs` (1), `tests/key_support/mod.rs`, `tests/composition_root.rs` (+1), `tests/{confirmation,command_tree,command_log,exit_codes}.rs` | 10 new, see Test Suite | None open |

Owner decisions of 2026-11-17 (asked in this session): rotation is config-first, so `admin rewrap` takes `--from-key-id <id>` instead of `--new-key-file`; the target is the current key id from `load_and_verify`. The batch size is a fixed 100 rows per call.

`key-check`: one `load_and_verify`; prints `current_key_id` and `key_ids`; each `KeyError` exits 2 with its variant name as the code (the same names `serve` prints; `key_kind` is shared). `admin keycheck-init`: reads the configured `master_key_file` (key 1, as 04's `init_key_check` documents) and makes one `init_key_check` call; a missing file exits 2 `FileMissing` with no call. `admin rewrap --from-key-id N`: refuses (exit 3, no batch) an id that is not an old key of the ring; otherwise `rewrap_batch(N, current, 100)` until 0, a progress line on stderr each `cli_progress_interval`, SIGINT ends the loop between batches (exit 130), closing note "keep key N". Composition root: new `dyn MasterKeyring` over the keyring 04 holds (asserted in `composition_root.rs`).

Spec and docs updated: `AZ-3797_key_commands.md` (outcome, ACs, both flagged concerns resolved) and `operator_command_surface.md` row for `admin rewrap`.

## Code Review Verdict: PASS_WITH_WARNINGS

Warnings (recorded, none blocks):
- The key file bytes read by `keycheck-init` live in a plain `Vec` inside `contracts::MasterKey` and are not wiped on drop (the type belongs to contracts; 04 holds the same key in memory).
- AC-6's `ps` search is satisfied by construction (no command takes key bytes); the test searches stdout, stderr and the JSON log for hex and base64 of a known key.
- The owner-effect parts (wrong key leaves `.keycheck` unwritten, files unchanged, resume after a kill) stay with 04's own tests and E-31, as the spec says.

## Test Suite

- `sunnycloud-server` crate (the only crate whose code changed): 194 passed, 0 failed (`--no-fail-fast`). `tests/config_binary.rs` no longer lists `key-check` among commands that stop before configuration: it now needs `master_key_file` and the database keys, so it exits 3 without them.
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.
- Workspace: not re-run this batch (one full-suite gate at the end of the implementation phase).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Source of the `keycheck-init` candidate | AZ-3797 AC-2 | Configured `master_key_file`; 04's doc says the candidate is key 1 | clear |
| 2 | Several old keys in the ring | AZ-3797 | One run per old key (`--from-key-id`) | clear |

## Commit

`[AZ-3797] Key check, keycheck-init and rewrap commands` — Loop: 1
