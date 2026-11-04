# Helper: Rate limit (in-memory abuse limiter)

**Crate**: `sc_ratelimit` (server only). **Used by**: 01_identity (sign-in steps, device grant requests and approvals, invalid tokens), 06_sharing (wrong share passwords, invalid share tokens). Constructed once by the composition root (12_operator_cli binary) and injected; 11_http_api only maps the result to 429.

## Purpose
One implementation of the AC-20 abuse limits. Counters are in-memory token buckets (data model 12.4): a restart resets them, which is acceptable because device codes live at most 10 minutes and tokens carry at least 128 bits. There is no generic per-route limiter in v1.0.

## Interface

| Operation | Input | Output | Errors |
|-----------|-------|--------|--------|
| `AbuseLimiter::new` | `&dyn Clock`, `LimitTable` (from config) | `AbuseLimiter` | `ConfigError` on a zero capacity or window |
| `check` | `LimitId`, `&Subject` | `Verdict::Allowed` or `Verdict::Limited{retry_after}`; reads only, takes nothing | none |
| `record_failure` | `LimitId`, `&Subject` | `Verdict::Allowed`, `Verdict::Tripped{retry_after}` (this call exhausted the bucket: the caller writes one audit event), or `Verdict::Limited{retry_after}` (already empty) | none |
| `clear` | `LimitId`, `&Subject` | `()`; used after a successful step where the limit counts consecutive failures | none |

- `LimitId`: `SigninIp`, `DeviceGrantIp`, `DeviceApprovalSession`, `DeviceApprovalAccount`, `DeviceApprovalIp`, `InvalidTokenIp`, `SharePasswordLink`, `ShareTokenIp`. Each maps to the metric label in `observability.md`.
- `Subject`: `Ip(IpAddr)`, `Session(Uuid)`, `Account(Uuid)`, `Link(Uuid)`. Never a name, email or token.
- `LimitSpec { capacity, window }`: the bucket refills at `capacity / window`. Defaults are the AC-20 numbers (sign-in 10 per minute per IP; device approvals 5 per session, 20 per account per hour, 50 per IP per hour; wrong share passwords 10 per link per 10 minutes; invalid share tokens 100 per minute per IP); `DeviceGrantIp` and `InvalidTokenIp` take their numbers from config with the AC-20 values.

## Rules
- Callers call `check` before the costly step (provider call, Argon2id, lookup) and `record_failure` after a failure. A `Limited` result is returned before any work or any information about the subject is revealed.
- `Tripped` happens once per exhaustion: the caller audits it once (`AuditWriter::record_best_effort`), never per attempt (AC-20).
- State is a sharded map `(LimitId, Subject) -> bucket` behind mutexes; no lock is held across an `await`. A bucket that has refilled to capacity is dropped. At `ratelimit_max_entries` (default 100,000) the oldest idle buckets are evicted first.
- Time comes from the injected clock's monotonic reading, so tests advance it with `ManualClock`.
- `auth_rate_limited_total{limit}` is incremented on every `Limited` and `Tripped` result.
- v1.0 runs one API process, so per-process counters are exact; a second replica would multiply every limit.

## Tests
NFT-SEC-04, 05, 06 (429 with `Retry-After` at the documented thresholds), AC-20 limits per `LimitId`, unit tests with `ManualClock` (refill, trip once, eviction order, `clear`).

## Risks
- An attacker spraying many source IPs can fill the map and evict a victim's bucket; the cap and oldest-idle eviction bound memory, and the weakest case (a reset counter) is the same as a restart.
- Behind a misconfigured proxy every client shares one IP and one bucket; `doctor` warns when the trusted-proxy list is empty.
