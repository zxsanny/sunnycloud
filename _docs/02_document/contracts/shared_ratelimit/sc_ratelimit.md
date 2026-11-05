# Contract: sc_ratelimit — in-memory abuse limiter for the AC-20 failures

**Component**: shared/ratelimit (crate `sc_ratelimit`, server only)
**Producer task**: E-08/c01 — `_docs/02_tasks/todo/B01-19_sc_ratelimit.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-09/c01 (stats snapshot); other batches: 01 (E-15), 06 (E-16), 11 (E-21, maps to 429), 12 (E-22, constructs it once)
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

One token-bucket limiter for the defined abuse failures (bad sign-in steps, device grants and approvals, invalid tokens, wrong share passwords, invalid share tokens). There is no generic per-route limiter in v1.0; counters are per process and reset on restart (data_model §12.4).

Sources: `common-helpers/08_helper_ratelimit.md`, `epics/E-08_cross-cutting-abuse-rate-limiting.md`, `deployment/observability.md` (metric labels).

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `AbuseLimiter::new` | `fn new(clock: Arc<dyn Clock>, table: LimitTable) -> Result<AbuseLimiter, ConfigError>` | `ConfigError` (this crate's own type) on a zero capacity or window | sync |
| `check` | `fn check(&self, limit: LimitId, subject: &Subject) -> Verdict` (`Allowed` or `Limited{retry_after}`; reads only) | none | sync, O(1) |
| `record_failure` | `fn record_failure(&self, limit: LimitId, subject: &Subject) -> Verdict` (`Allowed`, `Tripped{retry_after}` when this call exhausted the bucket, `Limited{retry_after}` when already empty) | none | sync, O(1) |
| `clear` | `fn clear(&self, limit: LimitId, subject: &Subject)` (after a success where consecutive failures count) | none | sync |
| `stats` | `fn stats(&self) -> LimiterStats` (per `LimitId`: limited and tripped counts) | none | sync |

| Type | Shape |
|---|---|
| `LimitId` | `SigninIp`, `DeviceGrantIp`, `DeviceApprovalSession`, `DeviceApprovalAccount`, `DeviceApprovalIp`, `InvalidTokenIp`, `SharePasswordLink`, `ShareTokenIp` (each maps to one `limit` label of `auth_rate_limited_total`) |
| `Subject` | `Ip(IpAddr)`, `Session(Uuid)`, `Account(Uuid)`, `Link(Uuid)` — never a name, email or token |
| `LimitSpec` | `capacity, window` (refill `capacity / window`) |
| `LimitTable` | one `LimitSpec` per `LimitId`; defaults: sign-in 10/min per IP; device approvals 5 per session, 20 per account, 50 per IP per hour; wrong share passwords 10 per link per 10 min; invalid share tokens 100/min per IP; `DeviceGrantIp` and `InvalidTokenIp` from `device_grant_ip_limit`, `invalid_token_ip_limit` (AC-20 values as defaults) |
| `Verdict` | `Allowed`, `Tripped{retry_after}`, `Limited{retry_after}` |

## Invariants

- Time comes from the injected clock's monotonic reading only.
- `Tripped` happens exactly once per exhaustion, so the caller audits once (`abuse_trip` through `record_best_effort`).
- The bucket map never holds more than `ratelimit_max_entries` (default 100,000) buckets; the oldest idle buckets are evicted first; a bucket refilled to capacity is dropped.
- No lock is held across an `await` (the API is synchronous); per-call cost is O(1) with sharded mutexes.
- Callers call `check` before any costly or revealing work and `record_failure` after a failure.

## Non-Goals

- Not covered: calling the limiter and writing `abuse_trip` (E-15, E-16); mapping to 429 with `Retry-After` (E-21); per-principal long-poll hold caps (E-12); sharing counters across processes.

## Versioning Rules

- A new `LimitId` is minor (adds a metric label value); changing a default number is minor but must update the AC-20 trace; removing a `LimitId` is breaking.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| approvals | 6 wrong approvals from one session within an hour (`ManualClock`) | 6th is `Limited` with `retry_after` | AC-20, AC-57 |
| share-password | 11 wrong passwords on one link within 10 min | 11th is `Limited` | AC-20 |
| trip-once | exhaust a bucket, then 3 more failures | one `Tripped`, then `Limited` | |
| refill | `tick(window)` after exhaustion | `check` returns `Allowed` | |
| cap | `ratelimit_max_entries + 1000` distinct IPs | map size never above the cap; oldest idle evicted | |
| parallel | 1,000 concurrent `record_failure` on one bucket of capacity 10 | exactly one `Tripped` | |

## Open points (flagged in the producer task)

1. Helper 08 and E-08 call the construction error `ConfigError`, which is also `sc_config`'s type name. This contract keeps the spec name inside `sc_ratelimit`; a rename needs the owner's OK.
2. The sign-in and device-approval numbers have no config keys today (compiled AC-20 defaults); share-limit keys exist in helper 01 (`share_wrong_password_limit`, `share_wrong_password_window`, `share_invalid_token_limit`).
3. Helper 08 passes `&dyn Clock`; a limiter that lives for the whole process holds the clock, so this contract takes `Arc<dyn Clock>`.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
