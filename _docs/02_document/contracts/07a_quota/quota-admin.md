# Contract: quota-admin

**Component**: 07a_quota
**Producer task**: E-11/c03 — `_docs/02_tasks/todo/B02-18_quota_admin_lifecycle.md` (`view`, `views`, `set_limit`, `AccountLifecycleHook`); E-11/c04 — `B02-19_quota_recompute.md` (`recompute`) (both renamed to their AZ keys at Step 4)
**Consumer tasks**: E-14 transfer (`view` for the staged pre-check), E-15 identity (`initialize` at account creation), E-17 admin (`views` in the user list, `set_limit`, `erase_batch` through the deletion purger), E-21 http_api (own quota view), E-22 operator CLI (`admin user set-quota`, doctor drift check through `recompute`)
**Version**: 1.0.0
**Status**: draft (frozen when E-11/c04 is accepted)
**Last Updated**: 2026-11-04

## Purpose

What the admin-side quota calls return and change, and what the quota lifecycle hook does at account creation and erase. Signatures are declared in `contracts` part 1 (E-00/c03, `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md`). Source: `components/07a_quota/description.md` §2-3, §11.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `QuotaAdmin::view` | `account` -> `QuotaView{limit_bytes: Option<i64>, effective_limit, used, reserved}` | `NotFound` | async |
| `QuotaAdmin::views` | `&[account]` -> map account -> `QuotaView` | none | async |
| `QuotaAdmin::set_limit` | `account, Option<i64>` -> `()` | `NotFound`, `InvalidLimit` | async |
| `QuotaAdmin::recompute` | `account, live_bytes, reserved_bytes, repair: bool` -> `Drift{stored, computed}` | none | async, system scope |
| `AccountLifecycleHook::initialize` | `tx: &mut DbTx, account` -> `()` | `Unexpected` | async, caller's tx |
| `AccountLifecycleHook::erase_batch` | `account, limit` -> `EraseProgress{remaining}` | `Unexpected` | async |

## Invariants

- `effective_limit` is the account's `limit_bytes` when set, otherwise the installation default `default_quota`.
- `views` answers in one database round trip for up to 500 ids and omits unknown ids (no error).
- `set_limit(Some(n))` accepts `0 <= n <= 2^53` bytes; `0` means no new data. Anything else is `InvalidLimit`. `None` restores the installation default. Lowering a limit below current use is allowed: it blocks new reservations and deletes nothing. Each successful change is logged at INFO with ids only. Authorization is the caller's: only 07 (`Scope::Admin`) and the operator CLI reach `set_limit`.
- `recompute` compares the stored `used_bytes` with the supplied `live_bytes` and the stored `reserved_bytes` with the supplied `reserved_bytes`, returns both as `Drift{stored, computed}`, writes the supplied values only when `repair = true`, and logs one WARN per drift found (ids only). The caller supplies the truth (02 `LiveUsageSource`, 03 `ReservationSource`), so 07a imports neither. It is a doctor and test tool: never called on a request path.
- `initialize` inserts the account's row (limit unset, used 0, reserved 0) inside the caller's transaction; a rollback leaves no row.
- `erase_batch` deletes the row and returns `remaining = 0`; a repeated call and a call for an account that never had a row also return `remaining = 0` without error. Open reservations vanish with the row.

## Non-Goals

- Not covered: the ledger calls (`quota-ledger.md`); admin user listing and roles (07); HTTP field names of the quota view (11 OpenAPI).

## Versioning Rules

- **Breaking changes** (limit bounds, unknown-id handling of `views`, hook results) require a new major version and notice to E-14, E-15, E-17, E-21 and E-22.
- **Non-breaking additions** (a new optional view field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| init-commit-rollback | `initialize` committed; second account rolled back | row (unset, 0, 0); no row | IT-07a-1 |
| set-limit-below-use | used 600 MiB, `set_limit(Some(500 MiB))` | accepted, used unchanged, `reserve(1)` refused | IT-07a-6 |
| set-limit-invalid | `set_limit(Some(-1))`, unknown id | `InvalidLimit`, `NotFound` | IT-07a-6 |
| views-500 | 500 ids plus 3 unknown | 500 entries, unknown omitted, one statement | IT-07a-12 |
| erase-idempotent | `erase_batch` twice, unknown account | `remaining = 0` each time | IT-07a-13 |
| recompute-drift | stored used = live + 777 | `Drift{live + 777, live}`, row unchanged; repair sets it | IT-07a-14 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
