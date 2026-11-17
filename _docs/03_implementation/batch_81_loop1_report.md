# Batch Report

**Batch**: 81
**Tasks**: AZ-3794_identity_operator_commands
**Date**: 2026-11-17

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3794_identity_operator_commands | Done (AC-1 to AC-6, command side) | `identity_commands.rs` (new), `commands/admin.rs`, `dispatch.rs`, `lib.rs`, `root/domain.rs`, `user_commands.rs` (`failure` now `pub(crate)`), `tests/identity_commands.rs` (8), `tests/composition_root.rs` (+1), `tests/command_tree.rs`, `tests/config_binary.rs` | See Test Suite | None open |

Owner decisions of 2026-11-17 (asked in this session): `admin create` takes no `--note` or `--ttl` (the invite is valid for `invite_ttl`; `create_admin_invite()` has no such parameters), and `admin invite` takes `--admin <account>` because `InviteService` needs an admin account that the operator does not have (`created_by` is a foreign key to accounts).

`admin setup-token`: one `ensure_setup_token`; a token prints once with the hint `sign in with this token; it can be used once`; no token (an account exists) is `SetupNotAvailable`, exit 2, nothing on stdout. `admin create`: one `create_admin_invite`, token once. `admin reset <account>`: one `reset_admin`, token once; `NotFound` exits 2 `not_found`, `NotAnAdmin` exits 2 `not_an_admin`; the sole admin is allowed (G-12-3). `admin promote|demote <account>`: one `AdminService::update_user(&Actor::Operator, account, Some(role), None)`; `LastAdmin` exits 2 `last_admin` (shared `user_commands::failure`). `admin invite --admin A create [--note]|list|revoke <id>`: one `InviteService` call each; `list` prints id, note, created, expires and redeemed times and never a token; a revoke of an unknown invite exits 2 `not_found`. A bad account or invite id exits 3 before any call. Tokens go only through `OneTimeSecret`; tests assert they appear once on stdout and 0 times on stderr or in the JSON log, also after an unexpected failure.

Composition root: new services `dyn OperatorIdentityOps` and `dyn InviteService`, both the `IdentityInvites` that 01 holds (asserted in `composition_root.rs`).

Docs updated: `AZ-3794_identity_operator_commands.md`, `operator_command_surface.md` and `components/12_operator_cli/description.md`.

## Code Review Verdict: PASS_WITH_WARNINGS

Warnings (recorded, none blocks):
- The sign-in hint names no URL and no page: the sign-in UI is not part of this task and the documents do not fix its wording. It states only what the contracts guarantee (single use).
- `admin invite list` and `revoke` take `--admin` although 01 ignores it for those two calls (documented in the identity contract); it is required for a uniform command shape.
- Redemption and the owner-effect parts of IT-12-2 and IT-12-3 stay with AZ-3654, E-15/c08, E-17/c04 and FT-N-46, as the spec says.

## Test Suite

- `sunnycloud-server` crate (the only crate whose code changed), `--no-fail-fast`: 206 tests, 0 failed after one fix. The first run failed `config_binary::commands_that_use_no_provider_worker_token_or_master_key_get_past_configuration`, which listed `admin setup-token` among commands that stop before configuration; it now builds the identity service and needs the full server configuration like every other implemented command, so it was removed from that list (the test still covers `migrate` and an unfinished command).
- CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `sunnycloud-server` clippy `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `CI-parity: PASS`.
- Workspace: not re-run this batch (one full-suite gate at the end of the implementation phase).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `reset_admin` returns `NotAnAdmin` for a non-admin account | AZ-3794 concern 3 | Exit 2 `not_an_admin` | clear |
| 2 | Sign-in hint wording and URL | AZ-3794 AC-1 | Generic hint; the web page is another task | unclear (revisit with the SPA first-run task AZ-3654) |

## Commit

`[AZ-3794] Identity operator commands` - Loop: 1
