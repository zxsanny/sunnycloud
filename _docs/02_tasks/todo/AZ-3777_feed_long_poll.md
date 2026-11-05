# Long-poll hold: notifier, revalidation, safety recheck, shutdown, hold cap

**Task**: AZ-3777_feed_long_poll
**Name**: Feed long-poll hold
**Description**: Park empty feed reads without a database connection, wake them on commit within milliseconds, end them when the principal is revoked, heal lost wakes, and bound holds per principal
**Complexity**: 5 points
**Dependencies**: AZ-3773_feed_gaps_spike, AZ-3776_change_feed_read, AZ-3765_sc_clock, AZ-3786_sc_observability
**Component**: 05 sync_feed
**Tracker**: AZ-3777
**Epic**: AZ-3635 (E-12)
**Ref**: E-12/c05

## Problem

Seventy percent of sessions are idle long-polls (AC-1): they must not hold database connections. A commit on device A must wake device B's held read at once (AC-5's server share). A revoked session or device must stop syncing within 5 s even while its read is parked (AC-21). There is no PgListener (ADR-002, one API process), so a missed in-process wake or a change made by a CLI process must still be noticed.

## Outcome

- `FeedNotifier::notify(account, seq)` wakes that account's held reads without blocking; the per-account entry exists only while someone waits.
- A read whose first page is empty and whose `wait` (capped by `longpoll_wait_max`) is above 0 parks with no connection. It wakes on notify; every `longpoll_auth_recheck` it asks the passed `PrincipalRevalidator` and ends with `Unauthenticated` if revoked; every `feed_safety_recheck` it re-reads with one short query, counting `feed_missed_wake_total` and logging WARN when that finds changes a notify missed; on `wait` expiry it returns an empty page with the same cursor.
- A shutdown signal wakes every hold, which returns an empty page with the same cursor.
- A database outage during a hold returns a typed unavailable error (11 maps it to 503) and moves no cursor.
- A principal's fifth concurrent hold is refused with the error decided in E-12/c01 (`max_holds_per_principal` = 4).

## Scope

### Included
- The notifier map, the hold loop, the two recheck timers, shutdown, outage handling, the hold cap, the missed-wake metric and WARN.

### Excluded
- The read path and cursor rules (E-12/c04); the real 01 revalidator cases and AT-05-3 (E-12/c08); HTTP timeouts (E-21/c05) and connection draining (E-21/c17).

## Acceptance Criteria

**AC-1: Parked reads hold no connections**
Given 100 holders across principals of one account and of 10 accounts with `wait = 300 ms`
When they are parked
Then the service uses at most 4 database connections (one per safety recheck query at a time)

**AC-2: Notify wakes within milliseconds**
Given those holders
When a stamp, COMMIT and `notify` happen for one account
Then that account's holders return the new page within 50 ms of the notify and the others keep waiting

**AC-3: Expiry returns an empty page**
Given a hold with `wait = 300 ms` and no change
When the wait expires
Then an empty page with the same cursor returns at 300 ms plus or minus 100 ms

**AC-4: Lost and foreign wakes are healed**
Given `feed_safety_recheck = 200 ms`
When a stamp commits without `notify`, and another process commits a change directly
Then the first is returned by the safety recheck with `feed_missed_wake_total` increased by 1 and one WARN line, and the second is seen within the recheck interval plus 100 ms

**AC-5: Hold cap**
Given one principal with 4 parked reads
When it opens a fifth
Then the fifth is refused with the error decided in E-12/c01 and the 4 stay parked

**AC-6: Revocation ends a hold (scripted revalidator)**
Given `longpoll_auth_recheck = 100 ms` and a scripted revalidator that turns false after 150 ms, plus a second holder that stays valid
When both are parked
Then the first ends with `Unauthenticated` between 150 and 300 ms with no items in its response, and the second is unaffected

**AC-7: Shutdown does not wait for the timeout**
Given 100 holders with `wait = 30 s`
When shutdown is signalled
Then all 100 return within 200 ms as empty pages with their cursors unchanged

**AC-8: Database outage**
Given parked holds and the PostgreSQL container paused for 3 s
When a safety recheck hits the outage, and later the same cursor is read after resume
Then the in-flight reads return the typed unavailable error within the recheck interval plus 1 s, and the later read returns the changes made around the outage with no cursor movement

**AC-9: Notifier keeps no state**
Given no waiters
When 100,000 notifies for distinct accounts run
Then they take under 200 ms in total and RSS grows by at most 5 MB; with waiters, a notify wakes exactly that account's waiters

**AC-10: Many holders (PT-05-3)**
Given 100 and then 1,000 holders with `wait = 30 s` and a stamp every 2 s on 10 accounts for 2 minutes on REF-SRV
When the run finishes
Then holders use at most 5 database connections (failure 10), RSS per holder is at most 16 KiB (failure 64 KiB), and missed wakes are 0

**AC-11: Wake latency (PT-05-4)**
Given 100 holders on 100 accounts, one stamped commit per account every 500 ms for 5 minutes, and 100 background stamping streams on REF-SRV
When the run finishes
Then wake latency p95 is at most 50 ms (failure 250 ms) and p99 at most 150 ms (failure 500 ms)

## Non-Functional Requirements

**Performance**
- As AC-10 and AC-11; end-to-end budget 5 s (AC-5).

**Reliability**
- In-memory state only (the notifier map); after a restart held reads are gone and clients re-poll with the same cursor. Nothing to rehydrate.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-9 | Notifier entry removed when the last waiter leaves | Map empty after |
| AC-5 | Per-principal hold counter | Fifth refused; counter released when a hold ends |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 100 holders | IT-05-6 connections | <= 4 connections | AC-1, NFT-RES-21 |
| AC-2 | 100 holders, one notify | IT-05-6 wake | <= 50 ms for that account only | AC-5, FT-P-07 |
| AC-3 | `wait = 300 ms` | IT-05-6 timeout | Empty page, same cursor | |
| AC-4 | Stamp without notify; second process | IT-05-6 lost wake | Healed; metric +1; WARN | NFT-RES-20 |
| AC-5 | 5 holds of one principal | IT-05-6 hold cap (G-05-2) | Fifth refused | |
| AC-6 | Scripted revalidator | IT-05-7 (a) | `Unauthenticated` in 150-300 ms | AC-21 |
| AC-7 | 100 holders, `wait = 30 s` | IT-05-8 | All within 200 ms | |
| AC-8 | Paused PostgreSQL | IT-05-9 | Typed error; clean recovery | NFT-RES-20 |
| AC-9 | 100,000 notifies | IT-05-15 | < 200 ms; <= 5 MB | |
| AC-10 | 100 and 1,000 holders | PT-05-3 | <= 5 connections; <= 16 KiB each | NFT-RES-21 |
| AC-11 | 100 accounts, background load | PT-05-4 | p95 <= 50 ms | NFT-PERF-08, NFT-PERF-09 |

## Constraints

- No PgListener and no held connection or transaction while parked; each recheck takes one pooled connection for one query.
- Test timers in milliseconds with the manual clock (300 / 100 / 200 ms defaults of the component tests).
- PT rows on REF-SRV only; elsewhere `NOT-RUN(HW-REF)`.

## Risks & Mitigation

**Risk 1: Replicas do not share the notifier (R06)**
- *Risk*: a second API process only sees changes on the 10 s recheck, which breaks AC-5.
- *Mitigation*: one API process in v1.0 (decision 12); the notifier sits behind its port so a PgListener can replace it later.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Hold-cap error name and code wait for E-12/c01 (G-05-2) | Epic owner | open | Low |
| The typed error for a database outage during a hold is not named (only "maps to 503") | Epic owner | open | Low |

## Contract

This task produces/implements the `FeedNotifier` part of `_docs/02_document/contracts/05_sync_feed/feed-writer-ports.md` and the hold part of `_docs/02_document/contracts/05_sync_feed/change-feed.md`.
Consumers MUST read those files — not this task spec — to discover the interface.
