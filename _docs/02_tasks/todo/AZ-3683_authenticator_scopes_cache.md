# Authenticator: single-probe authentication, session cache, scopes and revalidation

**Task**: AZ-3683_authenticator_scopes_cache
**Name**: Authenticator and scope model
**Description**: Turn a session or device credential into a principal with one indexed read, answer every dead credential the same way, enforce the four scopes, and let held long-polls see a revocation within the cache bound
**Complexity**: 5 points
**Dependencies**: AZ-3682_identity_crate_migration_config, AZ-3768_contracts_part1, AZ-3764_sc_error, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base
**Component**: 01 identity
**Tracker**: AZ-3683
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c03

## Problem

Every request on the public listener must know who calls and whether that kind of credential may use the route. A revoked session or device must stop within 5 s, including held long-polls (AC-21). An attacker must not learn whether a token is unknown, expired or revoked from the answer or from the work done. Worker tokens must never open a user principal (AC-79).

## Outcome

- `Authenticator::authenticate` and `require`, and `PrincipalRevalidator::revalidate`, implemented on the ports defined in `contracts` (E-00/c03).
- One `Unauthenticated` answer for every dead credential, produced by the same single database read.
- Revocation seen within `session_cache_ttl` (2 s) plus 50 ms.
- The behavioural contract `_docs/02_document/contracts/01_identity/authenticator.md`, which E-21 (auth extraction) and E-12 (long-poll recheck) read.
- The hot path meets PT-01-1.

## Scope

### Included
- `authenticate(Credential::Session | Credential::Device)`: one read on the unique hash index of `sessions.token_hash` or `device_tokens.token_hash`, joined to the account state; expiry, revocation and account state (`disabled`, `deleting`) evaluated in code, not in the `WHERE` clause; result `Principal{account_id, kind, session_or_device_id, role, label}`.
- In-process cache of results, entries live at most `session_cache_ttl`, invalidated by age only.
- `revalidate(&Principal)`: true while the credential is still live, answered from the cache no older than `session_cache_ttl`.
- `require(&Principal, Scope)` for `Public`, `SessionOnly`, `SessionOrDevice`, `Admin`.
- The contract file.

### Excluded
- Reading cookies and bearer headers, 401/403 mapping (E-21/c07).
- The revocation operations themselves: `SessionService` (E-15/c06), `DeviceGrantService` (E-15/c07), `AccountAdmin::set_state` (E-15/c10). Each of those tasks adds its path to IT-01-2.
- `touch` / `last_seen_at` (E-15/c06); invalid-token abuse counting (E-15/c11); worker authentication (08 `WorkerAuthenticator`).

## Acceptance Criteria

**AC-1: One answer for every dead credential (AC-21, AC-59)**
Given account A with one session S and one device token D (fixture rows) and `session_cache_ttl = 0`
When `authenticate` is called with a never-issued token, S expired (clock past both TTLs), S revoked, D revoked, S of a `disabled` account, S of a `deleting` account, a malformed prefix and an empty string
Then all 8 calls return the same `Unauthenticated` value with no payload difference, and the statement log shows exactly one statement per call on a `*_hash` unique index and no write

**AC-2: Live credentials give a principal**
Given S and D before any revocation
When `authenticate` is called with each
Then it returns `Principal{account_id: A, kind, session_or_device_id, role, label}` with `kind` Session for S and Device for D

**AC-3: Revocation visible within the cache TTL (AC-21)**
Given `session_cache_ttl = 200 ms`, one session principal and one device principal with a warm cache
When each credential row is revoked through the fixture connection and `revalidate` is polled every 20 ms
Then `revalidate` is true before the revocation and false within 200 ms + 50 ms after it, and `authenticate` then returns `Unauthenticated`

**AC-4: Scope matrix without a worker scope (AC-57, AC-79)**
Given a member session, an admin session and a device principal
When `require` runs for each of the 4 scopes, and a worker-style bearer (`wk_...`) is passed to `authenticate` as a session and as a device credential
Then member: `Public`, `SessionOnly`, `SessionOrDevice` ok, `Admin` Forbidden; admin session: all ok; device: `Public` and `SessionOrDevice` ok, `SessionOnly` and `Admin` Forbidden; the worker-style bearer gets `Unauthenticated` in both variants

**AC-5: Hot path cost (PT-01-1)**
Given 100 principals on 16 tasks at 5,000 `authenticate` calls per second (95% repeat tokens) for 2 minutes after a 10 s ramp-up
When latency and resources are measured
Then cache-hit p95 is at most 0.2 ms (fail above 1 ms), cache-miss p95 at most 3 ms (fail above 8 ms), miss p99 at most 6 ms (fail above 15 ms), error rate 0%, CPU at most 1 core, DB connections at most 8, memory growth at most 20 MB

## Non-Functional Requirements

**Performance**
- PT-01-1 targets in AC-5. Env C-REF: off REF-SRV the functional part runs and the timing result is `NOT-RUN(HW-REF)`.

**Reliability**
- Revocation visible within `session_cache_ttl` + 50 ms; held polls end within 5 s with E-12's recheck (AC-21).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | `Credential` has exactly two variants (exhaustive match compiles) | No third variant exists |
| AC-4 | `require` scope table for every kind and role | Matches the matrix in AC-4 |
| AC-3 | Cache entry age with the manual clock | Entry older than `session_cache_ttl` is never served |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Account A, session S, device D, TTL 0 | IT-01-1: 8 dead credentials | Same `Unauthenticated`; 1 indexed read, 0 writes each | NFT-SEC-08 |
| AC-2 | Live S and D | IT-01-1 positive case | Principal with account, kind, role, label | |
| AC-3 | TTL 200 ms, warm cache | IT-01-2 (fixture revocation path) | Flip within 250 ms; then `Unauthenticated` | AC-21, NFT-SEC-08, FT-N-30 |
| AC-4 | Member, admin, device principals | IT-01-3 | Matrix as stated; worker bearer rejected | NFT-SEC-07, NFT-SEC-50 |
| AC-5 | 100 principals, 5,000 req/s, 2 min | PT-01-1 | Targets met (C-REF) | |

## Constraints

- One indexed probe per cache miss; the decision for revoked or expired is taken in code so the database path is the same for every dead credential (data model §4.3).
- Identity imports no other component crate (ADR-015); the port shapes come from `contracts`.

## Risks & Mitigation

**Risk 1: Cache TTL raised in production**
- *Risk*: a `session_cache_ttl` above 3 s breaks AC-21.
- *Mitigation*: start validation refuses it (E-15/c02 AC-5).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 01's `Scope` has 4 values and puts `DELETE /devices/me` under `SessionOrDevice`, while 11's `AuthClass` has a separate `DeviceOnly`, plus `Session`, `ShareGrant` and `Worker`. The contract records the 4 scopes and leaves the session refusal for `revoke_self` to `DeviceGrantService` (E-15/c07) unless the owner adds a `DeviceOnly` scope | E-15 and E-21 owners | open | Medium |
| `Principal` field names differ: 01 `session_or_device_id`, 11 `session_id`; the contract uses 01's | E-21 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/01_identity/authenticator.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
