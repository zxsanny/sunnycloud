# Sync feed security suite, revoked-device acceptance, metrics and logs

**Task**: AZ-3780_feed_security_acceptance
**Name**: Feed security, acceptance and logs
**Description**: ST-05-1..5, the revoked-device acceptance test AT-05-3 and the real-identity revalidation case IT-05-7 (b), plus the feed's metrics and log levels
**Complexity**: 3 points
**Dependencies**: AZ-3775_feed_stamp, AZ-3776_change_feed_read, AZ-3777_feed_long_poll, AZ-3779_epoch_admin, AZ-3683_authenticator_scopes_cache, AZ-3686_session_refresh_rotation, AZ-3687_device_grant_exact_once, AZ-3690_account_admin_and_guard, AZ-3786_sc_observability
**Component**: 05 sync_feed
**Tracker**: AZ-3780
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c08

## Problem

The feed carries file names and tree structure, so it must never leak another account's data through cursors or row access (AC-59), must not let one principal park unbounded reads, must treat cursor text as data, and must stop returning items once a principal is revoked (AC-21). Operators need a few metrics and log lines that carry no names or paths.

## Outcome

- ST-05-1 to ST-05-5 implemented at the port boundary against real PostgreSQL 18.
- IT-05-7 (b) and AT-05-3 run with the real 01 revalidator: session revoke, device revoke and account deletion end held reads in time.
- Metrics and logs as the description's logging section: ERROR on a stamp invariant violation, WARN with `feed_missed_wake_total` on a missed wake, INFO for each epoch bump (old, new), a `resync_required` count kept in metrics only, no names or paths in any feed log line.

## Scope

### Included
- The five security tests, IT-05-7 (b), AT-05-3, the `resync_required` counter, a log audit over the suite's runs.

### Excluded
- AT-05-1 (moved to E-12/c09 because its first step publishes through the real 02); the missed-wake counter itself (E-12/c05).

## Acceptance Criteria

**AC-1: Cursors and reads stay in their account (ST-05-1)**
Given account A at seq 80 and account B at seq 3 in one installation epoch
When B reads with A's cursor `E.50`, with `E.0`, takes its own head, and 1,000 random cursor pairs are tried
Then B gets `cursor_ahead` or only its own items, never one of A's node ids, names or counters, and `ResyncRequired` carries only `reason`

**AC-2: Row-level security (ST-05-2)**
Given the API role scoped to B
When it runs an unfiltered select, `UPDATE account_feeds SET seq = seq + 1000` and `ALTER TABLE` on the feed table
Then A's row is invisible and unchanged, the DDL is denied, and `bump_epoch` and `marker_status` are reachable only in system scope

**AC-3: Hold exhaustion (ST-05-3)**
Given one principal opening 1,000 reads with `wait = 30 s`, and 100 principals opening 10,000
When task count, memory and connections are sampled
Then at most 4 reads per principal are accepted, every further read is refused with `TooManyHolds` (429 `rate_limited` + `Retry-After`), RSS grows by at most 32 MB, connections stay within baseline + 2, and other principals are unaffected

**AC-4: Cursor injection (ST-05-4)**
Given the cursors `E.1; DROP TABLE account_feeds`, `E.1 OR 1=1`, Unicode digits and a 10 kB string
When each is read
Then each returns `BadCursor`, no SQL error surfaces and `account_feeds` is unchanged

**AC-5: Nothing after revocation (ST-05-5)**
Given a parked read and changes stamped every 20 ms
When its principal is revoked
Then the read ends with `Unauthenticated` and no page with items is returned after the revoke instant plus recheck plus cache time (target 0 after 400 ms; failure if any after 5 s)

**AC-6: Real revocations end holds (IT-05-7 b)**
Given held reads of a session, a device token and an account, `session_cache_ttl = 200 ms`, and a non-revoked holder
When the session is revoked, the device is revoked and the account's deletion begins
Then each held read ends within `session_cache_ttl + longpoll_auth_recheck + 100 ms` and the other holder is unaffected

**AC-7: Revoked device stops syncing (AT-05-3)**
Given device token D parked on the feed
When `revoke_device` runs through 01
Then D's read ends with `Unauthenticated` within `session_cache_ttl + longpoll_auth_recheck`

**AC-8: Metrics and logs**
Given the runs of this suite and of E-12/c03 to c07
When the logs and metrics are inspected
Then a stamp invariant violation logs ERROR, a missed wake logs WARN and counts `feed_missed_wake_total`, each bump logs INFO with old and new epoch, `resync_required` answers are counted in metrics and not logged per request, and no feed log line contains a node name or path

## Non-Functional Requirements

**Reliability**
- Revoked principals stop within 5 s (AC-21).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-8 | Log field set of each feed event | Ids only |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | A at 80, B at 3 | ST-05-1 | 0 foreign items over 1,000 pairs | AC-59, NFT-SEC-13 |
| AC-2 | Role scoped to B | ST-05-2 | A untouched; DDL denied | NFT-SEC-46 |
| AC-3 | 1,000 and 10,000 holds | ST-05-3 | <= 4 per principal; bounded memory | AC-1 |
| AC-4 | Injection strings | ST-05-4 | `BadCursor`; tables intact | |
| AC-5 | Revoke under stamping | ST-05-5 | 0 item pages after the bound | AC-21, NFT-SEC-08 |
| AC-6 | Real 01 revocations | IT-05-7 (b) | Each ends in time | FT-N-30 |
| AC-7 | Device token parked | AT-05-3 | `Unauthenticated` in time | AC-21 |
| AC-8 | Suite runs | Log and metric inspection | Levels and fields as listed | |

## Constraints

- Real PostgreSQL 18 and the real 01 revalidator for AC-6 and AC-7; scripted revalidators only where the test says so.

## Risks & Mitigation

**Risk 1: Cache delays revocation**
- *Risk*: 01's session cache keeps a revoked principal valid.
- *Mitigation*: the bound includes the cache TTL; `longpoll_auth_recheck` must stay below 5 s minus the cache TTL.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `resync_required` metric has no name in the specs | Epic owner / E-09 | open | Low |
| AC-6 and AC-7 use the real 01 implementation from 05's tests; whether the E-00 crate-graph test allows that as a dev-dependency is not stated | E-00 | open | Medium |
