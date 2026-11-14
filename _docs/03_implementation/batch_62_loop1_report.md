# Batch Report

**Batch**: 62
**Tasks**: AZ-3819_account_view_deletion_request (done), AZ-3702_share_account_erase_hook (done)
**Date**: 2026-11-14

Two parallel workers: contracts + 01 + 07 (AZ-3819) and 06 (AZ-3702). Owner decision 2026-11-14 for AZ-3819: one read-only method on 01's account admin port. The parent added `request_as_operator` to 11's two test stubs (recording and stub ports), recorded the owner's complexity-budget choices for the next batch (AZ-3754 order-sensitive request hash, AZ-3755 no suggested name for blob downloads, AZ-3700 no cache), reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3819_account_view_deletion_request | Done | contracts (additive, part 1 1.5.0): `AccountAdmin::profile(account) -> AccountProfile{display_name, identities}` (implemented in 01 with two read statements), `AccountDeletionService::request_as_operator(account)` (CC-2); `IdentityView` moved to `identity/model.rs` and re-exported. 07: `AccountService::view` (role from the principal, profile from 01, quota from 07a, root from 02) and `quota_view`; `request` (device token `Forbidden`, exact lowercase hyphenated own id or `ConfirmationMismatch`, both before any transaction; one account-scoped transaction: `begin_deletion` + `admin_action` audit with ids only; deadline from the injected clock). AC-1..AC-4 on real 01, 02, 07a (04 GC queue and 05 stamp under the commit): 3 credentials rejected by a fresh authenticator, data intact, publish refused by the guard |
| AZ-3702_share_account_erase_hook | Done | `PgShareLifecycle`: `initialize` writes nothing (checked with `txid_current_if_assigned()`), `erase_batch` deletes up to `limit` links (revoked ones too) in one account-scoped transaction and reports the rest: 120 links in batches of 50 report 70, 20, 0, 0; another account byte-identical; erased links open as `NotFound` |

## Parent changes

- 11 test harness: `request_as_operator` in `RecordingDeletion` and `AccountDeletionStub` (no HTTP route; the CLI is E-22/c09).
- Owner decisions appended to AZ-3754, AZ-3755, AZ-3700.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3471 passed, 1 failed, 23 ignored. The failure is `sunnycloud-server` `config_binary::serve_refuses_a_retention_above_ninety_days_naming_the_range` (the known listener-probe timing flake under load; server config untouched by this batch); it passed 3 of 3 runs alone, recorded as flaky, not blocking. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no sqlx macros, no migration in this batch.

Warnings (recorded, not fixed):
- `request_as_operator` on an unknown account answers `Unexpected` (the port has no `NotFound`); E-22 may want a typed variant (`account_services.md` open point 5).
- A repeat `request` on a `deleting` account succeeds with a deadline from the new call while 01 keeps the first `deletion_requested_at`.
- The user's own deletion request is audited through `Actor::Admin(principal)` (the stored row holds the user's ids only; the variant name is misleading).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | No port gave display name and identities | AZ-3819 flag (Medium) | Owner decision: `AccountAdmin::profile` | clear |
| 2 | `ConfirmationMismatch` has no code in the shared status table | AZ-3819 flag (Medium) | Proposal `confirmation_mismatch`, 422 (spec, `account_services.md` open point 4); HTTP mapping is E-21 | unclear |
| 3 | Last-admin guard not in the request yet | AZ-3819 | AZ-3818 | clear |
| 4 | 07 gains dev-dependencies on 01, 02, 04, 05, 07a | AZ-3819 | ADR-015 allows dev edges; `crate_graph` green | clear |
| 5 | The account erase also deletes revoked links | AZ-3702 | Documented in 06 description | clear |

## Commit

`[AZ-3819] [AZ-3702] Account view and deletion request, share erase` — Loop: 1

## Next Batch

AZ-3754 then AZ-3755 (03, one worker) and AZ-3700 (06), with the owner's option A.
