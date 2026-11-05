# Contract: quota-ledger

**Component**: 07a_quota
**Producer task**: E-11/c02 — `_docs/02_tasks/todo/B02-17_quota_ledger.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-13 tree (release and adjust inside the commit, delete and cascade transactions, E-13/c07), E-14 transfer (reserve at session start and staged upload, release on abort and expiry), E-22 pipelines (large upload, tree conflicts)
**Version**: 1.0.0
**Status**: draft (frozen when E-11/c02 is accepted)
**Last Updated**: 2026-11-04

## Purpose

The exact semantics of the three ledger calls that run inside other components' transactions. The trait signature and `QuotaError` variants are declared in `contracts` part 1 (E-00/c03, `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md`). Source: `components/07a_quota/description.md` §3, `data_model.md` §8.2.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `reserve` | `tx: &mut DbTx, account, bytes: i64` -> `Reserved` | `QuotaExceeded{limit, used, reserved}` | async, caller's tx |
| `release_reservation` | `tx: &mut DbTx, account, bytes: i64` -> `()` | none | async, caller's tx |
| `adjust_used` | `tx: &mut DbTx, account, delta: i64` -> `()` | `InvariantViolation` | async, caller's tx |

## Invariants

- The ledger never opens, commits or rolls back a transaction. Everything it changes rolls back with the caller's transaction or savepoint.
- `reserve` is one conditional update: it succeeds only when `used + reserved + bytes <= effective limit` (the per-account limit, or the installation default `default_quota` when the limit is unset). On refusal it returns `QuotaExceeded` with the row's limit, used and reserved values and changes nothing. Concurrent reservations can never push `used + reserved` above the limit.
- A successful `reserve` holds the quota row lock until the caller's transaction ends. A caller that needs to serialise its own check (03's open-session cap) runs it after `reserve` in the same transaction and rolls back on refusal. There is no lock-order contract with other components.
- `release_reservation` subtracts and clamps at 0; a release that would go negative is logged once at WARN with ids only and is not an error.
- `adjust_used` applies a signed delta. A result below 0 returns `InvariantViolation` and leaves the row unchanged; no database constraint text reaches the caller.
- Extreme inputs (`i64::MAX`, negative byte counts, sums past `i64::MAX`) return a typed error; the ledger never panics, wraps or stores a negative value.
- Every statement is scoped by `account_id`; a call never changes another account's row.
- **Caller obligations**: pass only sizes of current revisions of live files (replaced revisions in retention, tombstoned data, thumbnails and transcodes never count); release a session's reservation exactly once, by whoever deletes the session row; a deadlock between transactions is retried by the caller (02 retries once).

## Non-Goals

- Not covered: the session cap and the free-space floor (03 and 04 refuse before calling the ledger); computing live usage (02) or open reservations (03); the HTTP mapping of `QuotaExceeded` to 507 `quota_exceeded` (11).
- Not covered: admin views, limits and drift checks (`quota-admin.md`).

## Versioning Rules

- **Breaking changes** (a refusal that changes the row, lock duration, clamping rule) require a new major version and notice to E-13 and E-14.
- **Non-breaking additions** (a new error variant callers already treat as unexpected) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| reserve-fits | limit 1 GiB, used 300 MiB, `reserve(100 MiB)` twice | both `Reserved`; reserved 200 MiB | IT-07a-2 |
| reserve-refused | limit 1 GiB, used 800 MiB, reserved 100 MiB, `reserve(124 MiB)` | `QuotaExceeded{1 GiB, 800 MiB, 100 MiB}`, row unchanged | IT-07a-3 |
| reserve-race | 100 x `reserve(50 GiB)` at limit 1 TiB | exactly 20 `Reserved` | IT-07a-4 |
| lock-to-tx-end | tx A reserves and waits 500 ms; tx B reserves | B waits until A ends | IT-07a-5 |
| adjust-negative | used 30, `adjust_used(-31)` | `InvariantViolation`, used 30 | IT-07a-7 |
| release-clamp | reserved 60 MiB, `release_reservation(200 MiB)` | reserved 0, one WARN, no error | IT-07a-8 |
| savepoint | release + adjust, rollback to savepoint | row unchanged | IT-07a-9 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
