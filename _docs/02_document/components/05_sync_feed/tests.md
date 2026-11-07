# Test Specification — 05_sync_feed

Tier: component integration (`cargo test -p sync-feed`) at the public port boundary: `FeedStamp`, `FeedNotifier`, `FeedHorizon`, `ChangeFeed` (`read`, `head`), `EpochAdmin` (`bump_epoch`, `head_report`, `marker_status`), the epoch `DoctorCheck`, `AccountLifecycleHook`. Env class `C`; timing rows `C-REF`. System scenarios are referenced by ID, not restated.

05 owns `account_feeds`, `nodes.seq` and `nodes_feed_idx` but not node creation. Node rows for stamp tests are inserted by the test through the `sunnycloud_migrator` role (fixture SQL) or by the real 02 `TreeCommit` in the integration cases (marked "via 02"). `EpochMarker` is the real 04 implementation on a temp directory (file I/O is the behaviour under test). `PrincipalRevalidator` is a scripted implementation for deterministic cases and the real 01 implementation in IT-05-7. The clock is the manual test clock; `longpoll_wait_max`, `longpoll_auth_recheck`, `feed_safety_recheck` are millisecond values (300 / 100 / 200 ms by default in these tests).

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|------------------------------|----------|----------|
| AC-1 | 70% idle long-poll sessions without connections | IT-05-6, PT-05-3 | Covered here + NFT-PERF-01, NFT-RES-21 |
| AC-5 | Server share of sync latency: wake after COMMIT in ms | IT-05-6, PT-05-4, AT-05-1 | Covered here + FT-P-07, NFT-PERF-08, NFT-PERF-09 |
| AC-8 | Order is commit order | IT-05-1, IT-05-2, IT-05-3 | Covered here + FT-P-11 |
| AC-21 | Held streams end on revoke within 5 s | IT-05-7, AT-05-3 | Covered here + FT-N-30, NFT-SEC-08 |
| AC-35 | Catch-up after suspend | IT-05-3, IT-05-4 | Covered here (cursor catch-up) + FT-P-58, NFT-RES-16 |
| AC-41 | Epoch, horizon, future cursor give `resync_required` | IT-05-4, IT-05-10, IT-05-11, IT-05-12, AT-05-2 | Covered here + FT-P-13, FT-P-14, NFT-RES-07, NFT-RES-08 |
| AC-59 | Cross-account scoping of feed | ST-05-1, ST-05-2 | Covered here + NFT-SEC-13 |
| AC-65 | Epoch bump in restore runbook; doctor flags missing bump | IT-05-10, IT-05-14 | Covered here + NFT-RES-13 |
| AC-27 | Feed row removed at account erase | IT-05-13 | Covered here + FT-P-19, NFT-RES-30 |
| AC-69 | 50 concurrent commits, every change exactly once, in order | IT-05-1, IT-05-2, PT-05-1 | Covered here (stamp/read half) + NFT-RES-35 (with 02, 03) |

## Blackbox Tests

### IT-05-1: Stamp semantics
**Summary**: one stamp = one counter increment and one `seq` for all touched nodes; a rollback leaves no gap.
**Traces to**: AC-69, AC-8
**Description**: account with feed row (epoch E, seq 0) and 600 node rows. Transactions: stamp 1 node; stamp 500 nodes (batch); stamp then `ROLLBACK`; stamp on an account without a feed row.
**Input data**: `FeedStamp::stamp(&mut tx, account, TouchedNodes{ids})`.
**Expected result**:
```
Returns Seq 1, then 2; nodes touched carry that seq (the 500 share seq 2); untouched nodes keep older seq
After the rolled-back stamp: account_feeds.seq unchanged and the next committed stamp returns the next integer (no gap)
Account without feed row -> AccountFeedMissing (typed, not panic)
Statement log: stamp is ONE data-modifying statement (single CTE)
```
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-05-2: 50 concurrent stamping transactions
**Summary**: concurrent writers on one account produce `seq` 1..N exactly once.
**Traces to**: AC-69
**Description**: 50 tasks at a barrier; each opens a transaction, writes a node fixture row, calls `stamp` as the last statement, COMMIT; 20% of tasks ROLLBACK instead; repeated 100 rounds. A reader loops `read` from `E.0`.
**Expected result**:
```
Committed stamps return distinct, contiguous values; final account_feeds.seq == number of committed stamps; no value skipped or repeated (set of nodes.seq == 1..N)
Reader pages: seq strictly non-decreasing between pages, every committed node appears once with its latest state, no node missing at the end
0 deadlocks, 0 serialization errors surfaced
```
**Max execution time**: 120 s
**Dependencies**: PostgreSQL. Via 02: IT-02-13 repeats this through `publish`.

### IT-05-3: read pages, projection and boundaries
**Summary**: `read` returns the latest state per node ordered by `seq, id`, ends pages on `seq` boundaries.
**Traces to**: AC-8, AC-35
**Input data**: node A touched at seq 1 and again at seq 5 (renamed); folder F tombstoned at seq 6; batch of 500 nodes at seq 7; `limit = 100`; cursor `E.5`.
**Expected result**:
```
From E.0: A appears once with seq 5 and the new name; F state tombstone; ChangeItem fields node_id, parent_id, kind, name, state, revision_id?, size?, content_hash?, seq all present
Page containing seq 7 returns all 500 items (exceeds limit), has_more false after it; next_cursor = E.7; resuming from E.7 returns an empty page
cursor E.5 returns only seq > 5 (seq 6, 7); a folder that turns tombstone -> live shows state live at the new seq
```
**Max execution time**: 8 s
**Dependencies**: PostgreSQL

### IT-05-4: Cursor validation
**Summary**: every invalid or stale cursor is classified.
**Traces to**: AC-41, AC-35
**Description**: feed epoch E, seq 50, purge_horizon 20.
**Input data**: `E+1.10`, `E.19`, `E.20`, `E.50`, `E.51`, `E.9999999999`, strings `""`, `abc`, `1.`, `.5`, `1.2.3`, `-1.5`, `99999999999999999999.1`.
**Expected result**:
```
E+1.10 -> ResyncRequired{epoch_changed}; E.19 -> ResyncRequired{below_purge_horizon}; E.20 -> ok (S == horizon is allowed); E.50 -> empty page
E.51 and E.9999999999 -> ResyncRequired{cursor_ahead}; malformed strings -> BadCursor (code invalid_cursor); none is a SQL error
```
**Max execution time**: 4 s
**Dependencies**: PostgreSQL

### IT-05-5: head
**Summary**: `head` returns the current `epoch.seq`.
**Traces to**: AC-41
**Expected result**: head after k stamps equals `E.k`; a new device takes head, lists the tree through 02, then `read(head)` sees only later changes; head of another account is independent.
**Max execution time**: 3 s
**Dependencies**: PostgreSQL

### IT-05-6: Hold, wake, timeout, no connection while parked
**Summary**: an empty read parks without a DB connection and wakes on notify within milliseconds.
**Traces to**: AC-1, AC-5
**Description**: 100 holders (distinct principals of one account and of 10 accounts), `wait = 300 ms`; then a stamp + COMMIT + `notify`; then a stamp without `notify` (lost wake).
**Expected result**:
```
While parked: pg_stat_activity shows <= 4 connections from the service (safety rechecks take one pooled connection for one query each); 100 holders do not hold 100 connections
notify -> holders of that account return the new page within 50 ms of notify; other accounts keep waiting
timeout -> empty page, same cursor, at 300 ms +/- 100 ms
lost wake: returned by feed_safety_recheck (200 ms) with metric feed_missed_wake_total +1 and a WARN log
a change committed by a second process (psql-style) is seen within feed_safety_recheck + 100 ms
holds above max_holds_per_principal (4): the 5th returns Err(TooManyHolds{retry_after}) at once (no parking, no connection), mapped on the wire to 429 `rate_limited` with a Retry-After header (proposed 5 s); the 4 parked reads of that principal stay parked and wake normally; once one of the 4 ends, a new read parks
```
**Max execution time**: 20 s
**Dependencies**: PostgreSQL

### IT-05-7: Revalidation ends a hold
**Summary**: a revoked principal's held read ends with `Unauthenticated` within the recheck interval.
**Traces to**: AC-21
**Description**: (a) scripted revalidator flips to false after 150 ms with `longpoll_auth_recheck = 100 ms`; (b) real 01: session revoked and device revoked and account `begin_deletion` while held, `session_cache_ttl = 200 ms`.
**Expected result**: (a) `Err(Unauthenticated)` between 150 and 300 ms; (b) each ends within `session_cache_ttl + longpoll_auth_recheck + 100 ms`; a non-revoked holder in the same run is unaffected; no items leaked in the final response.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL, 01 (real, case b)

### IT-05-8: Shutdown wakes every hold
**Summary**: graceful stop does not wait for the hold timeout.
**Traces to**: AC-1
**Expected result**: with `wait = 30 s` and 100 holders, signalling shutdown returns all 100 reads within 200 ms as empty pages with the same cursor.
**Max execution time**: 3 s
**Dependencies**: PostgreSQL

### IT-05-9: Database outage during a hold
**Summary**: a DB outage yields a typed unavailable error, not a hang or panic, and recovery is clean.
**Traces to**: AC-5
**Description**: pause the Postgres container for 3 s during holds, resume.
**Expected result**: reads in flight at a safety recheck return a typed error (maps to 503 in 11) within `feed_safety_recheck + 1 s`; after resume a new read with the same cursor succeeds and returns changes made during the outage; no cursor movement. System: NFT-RES-20.
**Max execution time**: 20 s
**Dependencies**: PostgreSQL container with pause/unpause

### IT-05-10: bump_epoch and marker
**Summary**: the epoch only grows, every row is bumped in one transaction, and the marker follows.
**Traces to**: AC-41, AC-65
**Description**: 3 accounts with epochs E; clock at `T`; call `bump_epoch(now)` three times; once with the marker write failing (read-only root); once with a clock earlier than E.
**Expected result**:
```
new_epoch = greatest(E+1, unix_seconds(now)) for all rows, identical across accounts; one transaction (abort mid-way leaves none changed)
EpochMarker::read() == new_epoch after success; marker write failure -> error returned AFTER the DB commit; rerun succeeds and raises the epoch again (idempotent in effect: no decrease)
clock earlier than E: epoch = E+1, never lower
every old cursor -> ResyncRequired{epoch_changed}; new cursor E'.0 reads normally; seq values unchanged
```
**Max execution time**: 8 s
**Dependencies**: PostgreSQL, 04 `EpochMarker`

### IT-05-11: Restore without bump (cursor ahead) and marker status
**Summary**: a restored older database is detected by cursors and by the marker.
**Traces to**: AC-41
**Description**: DB at T0 (seq 10); activity to seq 40; marker written 40's epoch (same E). `pg_dump` taken at T0, restored into a fresh database; device cursor `E.40`.
**Expected result**: `read(E.40)` -> `ResyncRequired{cursor_ahead}`; `marker_status()` -> `{db_epoch, marker: Present(epoch)}`; after the operator bump: `db_epoch > marker`-ordering consistent with `serve` refusal rule in 12 (value returned only; refusal itself is 12); all cursors resync. System: NFT-RES-07, FT-P-13.
**Max execution time**: 20 s
**Dependencies**: PostgreSQL (dump/restore inside container), 04

### IT-05-12: FeedHorizon
**Summary**: `raise_purge_horizon` is a non-decreasing UPDATE (`GREATEST(purge_horizon, $seq)`) in the caller's transaction.
**Traces to**: AC-41
**Expected result**: horizon raised to seq 30 -> cursors below it resync; zero matching rows (account being erased) -> `Ok`, no error; rollback of the caller tx undoes the raise; lower seq: with the horizon at 30, a call with 20 returns `Ok` and the horizon stays 30 (a cursor at seq 25 still gets `ResyncRequired{below_purge_horizon}`), and a call with 30 keeps it at 30.
**Max execution time**: 4 s
**Dependencies**: PostgreSQL

### IT-05-13: AccountLifecycleHook
**Summary**: feed row created with the installation epoch and erased.
**Traces to**: AC-27, AC-41
**Description**: empty install: first `initialize`; second account; `erase_batch`.
**Expected result**: first account: epoch = clock seconds and `EpochMarker` written; second: epoch = `max(epoch)` (same installation epoch), marker unchanged; `erase_batch` -> row deleted, `remaining = 0`, repeated call `remaining = 0`; a held read of the erased account ends via the revalidator.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL, 04

### IT-05-14: Epoch doctor check
**Summary**: `DoctorCheck` fails on a missing or mismatching marker.
**Traces to**: AC-65
**Input data**: marker absent + blobs present; marker 5 vs DB `max(epoch)` 9; equal; marker absent on an empty install.
**Expected result**: Fail with hint `admin epoch-bump`, Fail, Ok, Ok.
**Max execution time**: 3 s
**Dependencies**: PostgreSQL, 04

### IT-05-15: Notifier lifecycle
**Summary**: `notify` never blocks and does not accumulate state.
**Traces to**: AC-1
**Expected result**: 100,000 `notify` calls for distinct accounts with no waiters take < 200 ms total and grow RSS by <= 5 MB (map entries dropped); notify with waiters wakes exactly that account's waiters.
**Max execution time**: 5 s
**Dependencies**: none (in-process)

## Performance Tests

Architecture §6 gives sync latency (<= 5 s p95, <= 10 MB) and 100 sessions; the targets below are the server's derived share.

### PT-05-1: Stamp contention on one account
**Summary**: the counter row is the serialisation point.
**Traces to**: AC-69, AC-5
**Load scenario**:
- Concurrent users: 50 writers on one account
- Request rate: closed loop (stamp-last transactions)
- Duration: 2 minutes

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Throughput | >= 1,000 stamps/s | 300 stamps/s |
| Latency p95 | <= 50 ms | 150 ms |
| Latency p99 | <= 100 ms | 300 ms |
| Gaps/duplicates | 0 | 0 |
**Resource limits**: DB connections <= 50 (one per writer). Env C-REF (system: NFT-PERF-09).

### PT-05-2: Feed read cost
**Summary**: `seq > S` query stays on `nodes_feed_idx`.
**Traces to**: AC-5, AC-35
**Load scenario**: account with 1,000,000 nodes; 500-item pages; 20 concurrent readers; 5,000 reads; also catch-up of 500 items after a 30-minute gap.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Page latency p95 | <= 20 ms | 60 ms |
| EXPLAIN | index scan on `nodes_feed_idx`, no Seq Scan | any Seq Scan |
**Resource limits**: connections <= 8. Env C-REF.

### PT-05-3: 100 held polls
**Summary**: many holders use few connections.
**Traces to**: AC-1
**Load scenario**: 100 and 1,000 holders, `wait = 30 s`, steady for 2 minutes with a stamp every 2 s for 10 accounts.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| DB connections in use by holders | <= 5 | 10 |
| RSS per holder | <= 16 KiB | 64 KiB |
| Missed wakes (`feed_missed_wake_total`) | 0 | 0 |
**Env**: C-REF (system: NFT-RES-21).

### PT-05-4: Wake latency
**Summary**: COMMIT to held read returning, server side only.
**Traces to**: AC-5
**Load scenario**: 100 holders on 100 accounts, one stamped commit per account every 500 ms for 5 minutes, background write load of 100 stamping streams.

| Metric | Target (derived; budget is 5 s end to end) | Failure Threshold |
|--------|--------|-------------------|
| Wake latency p95 | <= 50 ms | 250 ms |
| Wake latency p99 | <= 150 ms | 500 ms |
**Env**: C-REF.

## Security Tests

### ST-05-1: Feed scoping and cursor reuse across accounts
**Summary**: account B reading with A's cursor never sees A's data.
**Traces to**: AC-59
**Attack vector**: reuse A's cursor, `head`, and held read with A's principal id mismatch.
**Test procedure**:
1. Accounts A (seq 80) and B (seq 3, same installation epoch).
2. B calls `read` with A's cursor `E.50` and with `E.0`; `head` for B.
3. Inspect the items and errors.
**Expected behavior**: B receives `cursor_ahead` resync (S > B.seq) or B's own items only; items never include A's node ids, names or counters; `ResyncRequired` carries only `reason`.
**Pass criteria**: 0 foreign node ids over 1,000 random cursor pairs. **Fail criteria**: any foreign item or foreign seq value.

### ST-05-2: Row-level security with the runtime role
**Summary**: role `sunnycloud` scoped to B cannot read or update A's `account_feeds`/`nodes.seq`.
**Traces to**: AC-59
**Attack vector**: raw `UPDATE account_feeds SET seq = seq + 1000`, `SELECT` without filter, `ALTER TABLE`.
**Test procedure**: execute under B's scope with the API pool.
**Expected behavior**: A's row untouched/invisible; DDL denied (NFT-SEC-46). `bump_epoch`/`marker_status` run only in system scope (operator path).
**Pass criteria**: A's feed row identical afterwards. **Fail criteria**: any change.

### ST-05-3: Hold exhaustion
**Summary**: one principal cannot park unbounded requests.
**Traces to**: AC-1, AC-59
**Attack vector**: 1,000 concurrent `read(wait = 30 s)` from one principal; 10,000 from 100 principals.
**Test procedure**: open them all; sample task count, memory and DB connections.
**Expected behavior**: per-principal cap 4 enforced, every read above it refused at once with `TooManyHolds` (wire 429 `rate_limited` + `Retry-After`); total memory bounded; DB connections unchanged; others unaffected.
**Pass criteria**: <= 4 accepted per principal, every other read is `TooManyHolds` / 429 `rate_limited` with a `Retry-After` header, RSS growth <= 32 MB, connections <= baseline + 2. **Fail criteria**: otherwise.

### ST-05-4: Cursor injection
**Summary**: crafted cursor strings are data.
**Traces to**: AC-59
**Attack vector**: `E.1; DROP TABLE account_feeds`, `E.1 OR 1=1`, unicode digits, 10 kB string.
**Test procedure**: call `read` with each.
**Expected behavior**: `BadCursor` (422 invalid_cursor); tables intact.
**Pass criteria**: 0 SQL errors surfaced, `account_feeds` unchanged. **Fail criteria**: any 5xx-class error.

### ST-05-5: Revoked principal gets nothing after revocation
**Summary**: the held loop never returns items after `revalidate` is false.
**Traces to**: AC-21
**Attack vector**: stolen revoked token on an open long-poll while changes keep arriving every 20 ms.
**Test procedure**: revoke; keep stamping.
**Expected behavior**: the read ends `Unauthenticated`; no page with items is returned after the revoke instant + recheck + cache.
**Pass criteria**: 0 item pages after revoke + 5 s (target 0 after 400 ms). **Fail criteria**: any.

## Acceptance Tests

### AT-05-1: A commit wakes B
**Summary**: device B's held poll sees device A's change at once.
**Traces to**: AC-5, AC-8
**Preconditions**: one account; B parked with `wait = 300 ms` at head.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | via 02: A publishes a file | stamp seq+1, COMMIT, notify |
| 2 | B's read returns | page with the node at the new seq within 50 ms |

### AT-05-2: Restore then resync
**Summary**: operator bump forces every device to resync.
**Traces to**: AC-41, AC-65
**Preconditions**: devices D1, D2 with cursors at seq 40; DB restored to seq 10.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | D1 reads `E.40` | `cursor_ahead` |
| 2 | `bump_epoch` | marker updated |
| 3 | D1 and D2 read old cursors | `epoch_changed`; new head cursor works |

### AT-05-3: Revoked device stops syncing
**Summary**: a held read ends when the device is revoked.
**Traces to**: AC-21
**Preconditions**: device token D parked.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `revoke_device` via 01 | |
| 2 | wait for D's read | `Unauthenticated` within `session_cache_ttl + longpoll_auth_recheck` |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| Node fixtures | N node rows per account inserted by the migrator role (folders, files, tombstones) | generated by factory | 600 to 1,000,000 rows (PT-05-2) |
| DS-02 | listing-10k (for catch-up pages) | generated | 10 MiB |
| Cursor corpus | valid, stale, ahead and malformed cursor strings | static file in the test crate | < 10 KB |
| Dump of T0 | `pg_dump -Fc` of a seeded database at seq 10 | generated per run inside the container | < 5 MB |

**Setup procedure**:
1. One `postgres:18@sha256:<pin>` per binary; `sunnycloud migrate` as `sunnycloud_migrator` into `tmpl`; per test `CREATE DATABASE ... TEMPLATE tmpl`.
2. Component connects as `sunnycloud`; fixtures and probes as `sunnycloud_migrator`; temp blob root for `EpochMarker`.
3. Manual `Clock`; hold, recheck and safety-recheck intervals in milliseconds.

**Teardown procedure**: drop database and temp root; assert no leftover tasks (all holders returned) and connection baseline.

**Data isolation strategy**: database per test from the migrated template (multi-connection and dump/restore cases cannot use transaction rollback); PT-05-2 uses a pre-built template `tmpl_1m` shared read-only and cloned per run.
