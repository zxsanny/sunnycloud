# Test Specification — 07a Quota

Scope: ports `QuotaLedger` (`reserve`, `release_reservation`, `adjust_used`), `QuotaAdmin` (`view`, `views`, `set_limit`, `recompute`) and `AccountLifecycleHook` (`initialize`, `erase_batch`) over the single `quota` table (`account_id` PK, `limit_bytes`, `used_bytes`, `reserved_bytes`, CHECK >= 0). 07a requires no component port, so the harness supplies `live_bytes` and `reserved_bytes` to `recompute` directly. Tests run in the harness superuser-assisted style described under Test Data Management; system scenarios are cited by ID, not restated.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-39 | 100 sessions declaring 50 GiB are refused beyond cap and quota (quota side) | IT-07a-3, IT-07a-4, IT-07a-5, AT-07a-1; sys NFT-RES-05, NFT-RES-LIM-01 | Covered by this file and system scenarios (session cap and disk floor are 03/04) |
| AC-74 | After 1,000 concurrent commits and deletes, usage equals the sum of live revisions and never exceeds the quota (owner) | IT-07a-10, IT-07a-11, PT-07a-1; sys NFT-RES-38 | Covered by this file (ledger side) and sys NFT-RES-38 (full commit path) |
| AC-68 | Reserved quota is released at session expiry (partly) | IT-07a-8, IT-07a-9; sys NFT-RES-34, FT-N-40 | Covered by this file (release primitive) and system scenarios (expiry timing is 03) |
| AC-27 | Account deletion erases the ledger row | IT-07a-13; sys FT-P-19, NFT-SEC-27 | Covered by this file and system scenarios |
| AC-1 | Scale (views for 500 accounts) | IT-07a-12; sys NFT-PERF-01 | Covered by this file (views part) and sys NFT-PERF-01 |

Counting rule under test (confirmed): only current revisions of live files count; replaced revisions, tombstones, thumbnails and transcodes never count. The ledger has no API to count derived data, so this is checked through the stack in AT-07a-2.

## Blackbox Tests

Harness `H1`: Rust integration binary against real PostgreSQL 18, role `sunnycloud`; each case opens its own `DbTx` the way 02/03 do. Config: `default_quota_bytes = 10 GiB`.

### IT-07a-1: Lifecycle hook creates the row
**Summary**: `initialize(tx, account)` inserts the `quota` row.
**Traces to**: AC-27
**Input data**: new account id inside a transaction; commit, then a second account whose transaction rolls back.
**Expected result**: committed account has `limit_bytes IS NULL`, `used_bytes = 0`, `reserved_bytes = 0`; rolled-back account has no row; `view` returns `effective_limit = 10 GiB`.
**Max execution time**: 2 s
**Dependencies**: PostgreSQL

### IT-07a-2: Reserve within the limit
**Summary**: `reserve` increases `reserved_bytes` and returns `Reserved`.
**Traces to**: AC-39
**Input data**: limit 1 GiB, used 300 MiB; `reserve(100 MiB)`, then `reserve(100 MiB)` again.
**Expected result**: both `Reserved`; `reserved_bytes = 200 MiB`, `used_bytes = 300 MiB`; `view` shows the same.
**Max execution time**: 2 s
**Dependencies**: PostgreSQL

### IT-07a-3: Reserve beyond the limit is refused and changes nothing
**Summary**: `reserve` returns `QuotaExceeded{limit, used, reserved}` and leaves the row untouched.
**Traces to**: AC-39
**Input data**: limit 1 GiB, used 800 MiB, reserved 100 MiB; `reserve(124 MiB)` (1 MiB too many), `reserve(123 MiB)` (exact fit to limit), `reserve(1)`.
**Expected result**: first call `QuotaExceeded{limit: 1 GiB, used: 800 MiB, reserved: 100 MiB}` and row unchanged; second call `Reserved` (used + reserved = limit exactly); third `QuotaExceeded`.
**Max execution time**: 2 s
**Dependencies**: PostgreSQL

### IT-07a-4: Reservation race, 100 sessions of 50 GiB
**Summary**: concurrent reservations cannot exceed the limit.
**Traces to**: AC-39
**Input data**: limit 1 TiB, used 0; 100 tasks, each in its own transaction, `reserve(50 GiB)`; a sampler reads the row every 5 ms.
**Expected result**: exactly 20 `Reserved` and 80 `QuotaExceeded` (20 x 50 GiB = 1000 GiB; the 21st does not fit); `reserved_bytes` at every sample <= 1 TiB; final `reserved_bytes = 1000 GiB`; 0 database errors other than a retried deadlock.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL

### IT-07a-5: Reservation held to the end of the caller's transaction
**Summary**: the `reserve` row lock lets a caller serialize its own check after `reserve` and roll back (03's open-session cap pattern).
**Traces to**: AC-39
**Input data**: tx A: `reserve(1 GiB)` then sleeps 500 ms before commit; tx B: `reserve(1 GiB)` on the same account started 100 ms later; then tx A rolls back (second run: commits).
**Expected result**: B blocks until A ends (B completes at >= 400 ms after its start); after A rollback, `reserved_bytes` shows only B's reservation; after A commit, both counted (if the limit allows).
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-07a-6: Effective limit and `set_limit`
**Summary**: per-user limit overrides the installation default; `None` restores the default; lowering below use is allowed.
**Traces to**: AC-39
**Input data**: used 600 MiB; `set_limit(Some(500 MiB))`; `reserve(1)`; `set_limit(None)`; `set_limit(Some(-1))`; `set_limit(unknown id)`.
**Expected result**: limit 500 MiB accepted, `used_bytes` still 600 MiB (nothing deleted), `reserve(1)` is `QuotaExceeded`; `None` gives `effective_limit = 10 GiB`; `-1` returns `InvalidLimit`; unknown id `NotFound`; each success logs INFO with ids only.
**Max execution time**: 3 s
**Dependencies**: PostgreSQL
Limit `0` and `i64::MAX` behaviour is not in the description (gap G-07a-1); the test records the observed outcome as a baseline, not a pass/fail until specified.

### IT-07a-7: `adjust_used` signed deltas and invariant
**Summary**: positive and negative deltas apply; a result below zero is refused.
**Traces to**: AC-74
**Input data**: used 100; `adjust_used(+50)`, `adjust_used(-120)`, `adjust_used(-31)`.
**Expected result**: used 150, then 30; third call `InvariantViolation` and used stays 30; no CHECK constraint error leaks to the caller.
**Max execution time**: 2 s
**Dependencies**: PostgreSQL

### IT-07a-8: `release_reservation` clamps at zero
**Summary**: releasing more than reserved clamps to 0, returns Ok and logs a WARN.
**Traces to**: AC-68
**Input data**: reserved 100 MiB; `release_reservation(40 MiB)`, then `release_reservation(200 MiB)`.
**Expected result**: reserved 60 MiB, then 0; no error; one WARN log line with the account id and no byte values of other accounts.
**Max execution time**: 2 s
**Dependencies**: PostgreSQL, log capture

### IT-07a-9: Commit-shaped sequence rolls back as one
**Summary**: release plus adjust inside the caller's savepoint roll back together.
**Traces to**: AC-68, AC-74
**Input data**: reserved 10 MiB, used 0; tx: `release_reservation(10 MiB)`, `adjust_used(+10 MiB)`; case A commits, case B rolls back to a savepoint, case C aborts the whole tx.
**Expected result**: A: reserved 0, used 10 MiB; B and C: reserved 10 MiB, used 0 (unchanged); `used + reserved` never observed above the limit by a concurrent reader at read committed.
**Max execution time**: 3 s
**Dependencies**: PostgreSQL

### IT-07a-10: 1,000 concurrent commits and deletes keep the ledger exact
**Summary**: ledger invariants under the AC-74 workload.
**Traces to**: AC-74
**Input data**: one account, limit 200 MiB; 1,000 tasks in 20 waves of 50. Each task is one transaction modeling 02/03: `reserve(r)` then on success `release_reservation(r)` and `adjust_used(+r)` (a commit), or `adjust_used(-d)` for a previously committed size (a delete); sizes from a seeded generator (1 KiB to 4 MiB); a task that hits a deadlock retries once like 02. A harness-side ledger records every successful commit and delete.
**Expected result**: final `used_bytes` equals the harness ledger sum; `reserved_bytes = 0`; the sampler (every 5 ms) never saw `used_bytes > 200 MiB` or `used_bytes + reserved_bytes > 200 MiB`; 0 CHECK violations; `recompute(account, live_bytes = ledger sum, reserved_bytes = 0, repair = false)` reports `Drift{stored == computed}`.
**Max execution time**: 60 s
**Dependencies**: PostgreSQL

### IT-07a-11: Interleaved accounts do not interfere
**Summary**: concurrent work on accounts A and B changes only their own rows.
**Traces to**: AC-74
**Input data**: IT-07a-10 workload on A while B receives a fixed sequence (reserve 1 MiB, adjust +1 MiB, release 1 MiB) 100 times.
**Expected result**: B ends with used = 100 MiB, reserved = 0 irrespective of A's activity; no lock wait on B above 100 ms (sampled from `pg_locks` age).
**Max execution time**: 60 s
**Dependencies**: PostgreSQL

### IT-07a-12: `views` and `view`
**Summary**: batched usage lookup for the admin list.
**Traces to**: AC-1
**Input data**: 500 accounts with seeded limits and usage (DS-29 snapshot), one call `views(all 500 ids)`, one `views` with 3 unknown ids mixed in, `view(unknown)`.
**Expected result**: the map holds 500 entries equal to the seeded values; `view(unknown)` returns `NotFound`; behaviour of unknown ids inside `views` is "none" per description (no error), recorded as present or absent (gap G-07a-2); single round trip (one statement in `pg_stat_statements` delta).
**Max execution time**: 3 s
**Dependencies**: PostgreSQL (`pg_stat_statements`)

### IT-07a-13: Erase removes the row
**Summary**: `erase_batch` deletes the quota row and reports `remaining = 0`; open reservations vanish with it.
**Traces to**: AC-27
**Input data**: account with reserved 5 GiB; call `erase_batch(account, 50)` twice, and on a never-initialized account.
**Expected result**: first call `remaining = 0`, row gone; second call `remaining = 0`, no error; third (unknown) `remaining = 0`; other accounts untouched.
**Max execution time**: 2 s
**Dependencies**: PostgreSQL

### IT-07a-14: `recompute` finds and repairs drift
**Summary**: `recompute` compares stored with supplied truth.
**Traces to**: AC-74
**Input data**: harness raises `used_bytes` by 777 and lowers `reserved_bytes` by 5 (superuser UPDATE); call `recompute(account, live_bytes, reserved_bytes, repair=false)`, then `repair=true`, then again `repair=false`.
**Expected result**: first call returns `Drift{stored: live + 777, computed: live}` (reserved drift also reported) and the row is unchanged; second repairs the row to the supplied values; third reports no drift; one WARN log per drift detection with ids only; the call is never made on a request path (no `recompute` in `pg_stat_statements` during IT-07a-10).
**Max execution time**: 3 s
**Dependencies**: PostgreSQL

## Performance Tests

### PT-07a-1: Hot-row throughput
**Summary**: reserve, release and adjust on one account sustain commit rates seen in AC-40 and AC-1.
**Traces to**: AC-74, AC-39
**Load scenario**:
- Concurrent users: 64 transactions in flight on one account, then 64 accounts with one transaction each
- Request rate: closed loop, each tx = reserve + release + adjust
- Duration: 2 minutes
- Ramp-up: 10 s linear

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Latency p95, one hot account | 30 ms | 100 ms |
| Latency p95, 64 accounts | 5 ms | 20 ms |
| Throughput, one hot account | 500 tx/s | 200 tx/s |
| Error rate (excluding retried deadlocks) | 0% | 0% |

**Resource limits**:
- Database connections: 64; CPU on db host under 60% (REF-SRV). Targets derive from AC-40 (10,000 files in 60 s = 167 commits/s) with 3x headroom and are proposed values; off reference hardware timings are `NOT-RUN(HW-REF)`.

## Security Tests

### ST-07a-1: Every statement is scoped by account
**Summary**: operations on account A cannot read or change account B.
**Traces to**: AC-74
**Attack vector**: wrong account id passed by a caller bug; id confusion between accounts.
**Test procedure**:
1. Create A and B with distinct usage. Run each `QuotaLedger` and `QuotaAdmin` method for A.
2. Compare B's row byte for byte before and after.
3. Pass a random UUID as account for each method.
**Expected behavior**: B unchanged; unknown ids give `NotFound` (view, set_limit) or no effect (release, erase); no cross-account value in logs.
**Pass criteria**: 0 changed columns on B.
**Fail criteria**: any change.

### ST-07a-2: Integer overflow and extreme inputs
**Summary**: huge byte counts never wrap or panic.
**Traces to**: AC-39
**Attack vector**: `reserve(i64::MAX)`, `reserve(-1)`, `adjust_used(i64::MIN)`, two reservations summing past `i64::MAX`.
**Test procedure**: call each on an account with limit 1 GiB and with limit `Some(i64::MAX)`.
**Expected behavior**: `QuotaExceeded` or `InvariantViolation` or a typed validation error; never a negative stored value, a panic or a SQL overflow error text in the result.
**Pass criteria**: row valid and CHECK intact after each call; error is typed.
**Fail criteria**: panic, wrapped value, raw database message.

### ST-07a-3: Role privileges
**Summary**: the runtime role cannot alter the table.
**Traces to**: AC-74
**Attack vector**: DDL or `TRUNCATE` through the API role.
**Test procedure**: as role `sunnycloud`, run `ALTER TABLE quota DROP CONSTRAINT ...` and `TRUNCATE quota`.
**Expected behavior**: permission denied; (RLS policy behaviour is 01's helper 07; sys NFT-SEC-46 covers roles).
**Pass criteria**: both refused.
**Fail criteria**: either succeeds.

## Acceptance Tests

### AT-07a-1: Quota refusal leaves nothing reserved
**Summary**: 100 sessions declaring 50 GiB beyond quota are refused with nothing reserved.
**Traces to**: AC-39
**Preconditions**:
- Compose stack, account with limit 200 GiB, DS-26 style `perf-client session-open`.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Open 100 upload sessions of 50 GiB | at most the number allowed by quota and per-account session cap succeed; the rest get HTTP 507 `quota_exceeded` |
| 2 | `GET /api/v1/accounts/me/quota` | `reserved` equals the sum of the successful sessions; `used` 0 |
| 3 | Abort all sessions | `reserved` returns to 0 |

### AT-07a-2: Derived data does not count
**Summary**: thumbnails and transcodes do not change `used_bytes`; recompute reports injected drift.
**Traces to**: AC-74
**Preconditions**: Compose stack with a worker; DS-03 photos (20) and DS-07 clip uploaded to an account.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Record quota `used` after uploads, wait until all jobs finish (poll `thumbnail-states`) | `used` equals the sum of original sizes |
| 2 | Check `used` again after jobs | unchanged, byte for byte |
| 3 | Harness adds 777 to `used_bytes`, run `sunnycloud doctor --json` | quota drift finding for the account (12 IT-12-15 covers the exit code) |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| accounts-500 | DS-29 snapshot with seeded limits and usage | ctl `golden+accounts-500` | about 50 MB |
| sizes-seeded | 1,000 sizes from 1 KiB to 4 MiB for IT-07a-10 | generated, seed 20261103 | < 1 MB |
| photos-20, clip-compliant | DS-03 subset, DS-07 | DS-03, DS-07 | about 200 MB |

**Setup procedure**:
1. Start PostgreSQL 18 (pinned digest); `sunnycloud migrate` into template database `sc_template`.
2. Per test: `CREATE DATABASE t_<id> TEMPLATE sc_template`; the SUT connects as `sunnycloud`; the harness superuser connection only arranges drift and reads rows.
3. Seed accounts through `AccountLifecycleHook::initialize` inside a transaction (the real path), not by direct INSERT.

**Teardown procedure**:
1. Drop `t_<id>`; leak check that no `quota` rows remain for deleted accounts.

**Data isolation strategy**: one database per test, real PostgreSQL only (no in-memory stand-in: the conditional UPDATE and row locks are the behaviour under test); concurrency tests use separate connections from the pool, never one connection with interleaved statements.

## Spec gaps found

- G-07a-1: bounds of `InvalidLimit` are not defined (negative only? zero? above physical size?).
- G-07a-2: `views` with an unknown id: omitted from the map or error is not stated ("none" errors).
- G-07a-3: `QuotaExceeded` to HTTP 507 `quota_exceeded` is 11's mapping; the nothing-reserved guarantee is the 07a property tested here.
