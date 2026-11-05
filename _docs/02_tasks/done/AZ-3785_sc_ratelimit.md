# sc_ratelimit: in-memory abuse limiter for the AC-20 failures

**Task**: AZ-3785_sc_ratelimit
**Name**: sc_ratelimit `AbuseLimiter`, limit table, bucket map with eviction, stats snapshot
**Description**: One in-memory token-bucket limiter for the defined abuse failures, driven by the injected monotonic clock, bounded in memory and tripping exactly once per exhaustion
**Complexity**: 3 points
**Dependencies**: AZ-3763_sc_config, AZ-3765_sc_clock, AZ-3652_initial_structure
**Component**: shared/ratelimit (helper 08)
**Tracker**: AZ-3785
**Epic**: AZ-3620 (E-08)
**Ref**: E-08/c01

## Document Dependencies

- `_docs/02_document/contracts/shared_config/sc_config.md` — `device_grant_ip_limit`, `invalid_token_ip_limit`, `ratelimit_max_entries`, share limit keys, produced by E-01/c01.
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — monotonic clock and `ManualClock`, produced by E-03/c01.

## Problem

AC-20 defines abuse limits for specific failures, with HTTP 429 and `Retry-After` beyond them. There is deliberately no generic per-route limiter in v1.0. Identity and sharing both need the same limiter; two implementations would count differently, and an unbounded map would let an attacker spraying addresses exhaust memory.

## Outcome

- Identity (01) and sharing (06) share one limiter, constructed once by the composition root.
- Each documented limit trips at its AC-20 number, returns `Limited` with a retry delay afterwards, and refills on the clock.
- Memory never exceeds the configured bucket cap.
- Per-limit counts are available to the metrics collector.

## Scope

### Included
- `AbuseLimiter::new` with the limit table (zero capacity or window rejected), `check` (read only), `record_failure` (`Allowed`, `Tripped{retry_after}`, `Limited{retry_after}`), `clear`.
- The eight `LimitId`s with their AC-20 defaults; `DeviceGrantIp` and `InvalidTokenIp` and the share limits from config; each mapped to the `limit` label of `auth_rate_limited_total`.
- `Subject` of ip, session, account or link (never a name, email or token).
- A sharded bucket map: refill at `capacity / window`, full buckets dropped, oldest idle buckets evicted at `ratelimit_max_entries` (default 100,000); no lock across an `await`.
- A stats snapshot (per limit: limited and tripped counts) for E-09.
- Crate documentation of the limit table with the config key for each non-default number.

### Excluded
- Calling the limiter and writing `abuse_trip` (E-15, E-16, with E-07); mapping to 429 (E-21); long-poll hold caps (E-12); counters shared across processes.

## Acceptance Criteria

**AC-1: Device approval limits (AC-20, AC-57)**
Given a `ManualClock` and one session
When 6 wrong device approvals are recorded within one hour
Then the 5th `record_failure` returns `Tripped`, the 6th returns `Limited` with a positive `retry_after`, and a `check` after it is `Limited`; the per-account (20) and per-IP (50) limits behave the same at their numbers

**AC-2: Share password limit (AC-20)**
Given one share link
When 11 wrong passwords are recorded within 10 minutes
Then the 11th call is `Limited` (the 10th tripped the bucket)

**AC-3: Trip once**
Given a bucket of capacity 10
When 13 failures are recorded
Then exactly one result is `Tripped` and the 3 after it are `Limited`

**AC-4: Refill**
Given an exhausted bucket
When the clock ticks by the limit's window
Then `check` returns `Allowed`

**AC-5: Bounded memory**
Given `ratelimit_max_entries` set to 1,000
When failures are recorded for 2,000 distinct addresses
Then the map never holds more than 1,000 buckets and the evicted ones are the oldest idle ones

**AC-6: Concurrency**
Given one bucket of capacity 10
When 1,000 `record_failure` calls run in parallel
Then exactly one returns `Tripped`

**AC-7: Construction refused for a zero limit**
Given a limit table with a zero capacity or a zero window
When `AbuseLimiter::new` runs
Then it returns the construction error naming the limit

**AC-8: Stats**
Given 3 `Limited` and 1 `Tripped` results for `SigninIp`
When the stats snapshot is read
Then it shows them under `SigninIp` and nothing under other limits

## Non-Functional Requirements

**Performance**
- O(1) per call with sharded mutexes.

**Security**
- An attacker spraying addresses cannot grow memory beyond the cap; the weakest case equals a restart.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Every `LimitId` default against AC-20 | Trips at the documented count |
| AC-3 | Trip-once sequence | One `Tripped` |
| AC-4 | Refill with `ManualClock` | `Allowed` after the window |
| AC-5 | Eviction order at the cap; full-bucket drop | Cap held; oldest idle first |
| AC-6 | 1,000 parallel failures | One `Tripped` |
| AC-7 | Zero capacity or window | Rejected |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Sign-in and device flows (through E-15, E-21) | NFT-SEC-04, NFT-SEC-05 | 429 with `Retry-After` at the thresholds | Security |
| AC-2 | Share links (through E-16, E-21) | NFT-SEC-06, NFT-SEC-17 | 429 at the thresholds; uniform not-found otherwise | Security |

## Constraints

- One API process in v1.0 (ADR-002, R06): per-process counters are exact; a second replica would multiply every limit.
- Constructed by the composition root (12) and passed by `Arc`; no global.

## Risks & Mitigation

**Risk 1: Shared address behind a misconfigured proxy**
- *Risk*: all clients share one IP bucket.
- *Mitigation*: `doctor` warns when the trusted-proxy list is empty (E-22).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The construction error is called `ConfigError` in helper 08 and E-08, the same name as `sc_config`'s error type; kept inside this crate. | E-08 owner | open | Low |
| Sign-in and device-approval numbers have no config keys (compiled AC-20 defaults). | E-08 owner | accepted-risk | Low |
| Helper 08 passes `&dyn Clock`; a process-long limiter holds the clock, so the contract takes a shared clock handle. | E-08 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_ratelimit/sc_ratelimit.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
