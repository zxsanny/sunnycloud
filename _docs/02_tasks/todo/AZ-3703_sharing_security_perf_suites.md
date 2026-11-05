# Sharing security suite ST-06-1, 2, 3, 7, 8 and performance suite PT-06-1..3

**Task**: AZ-3703_sharing_security_perf_suites
**Name**: Sharing security and performance tests
**Description**: Prove at the sharing port boundary that accounts cannot touch each other's links, tokens are strong and never stored or logged, guessing yields nothing, visitors learn only four fields and the cross-account lookup cannot scan; and build the three performance scenarios
**Complexity**: 5 points
**Dependencies**: AZ-3697_share_create_list_revoke, AZ-3698_share_lookup_open_uniform, AZ-3699_share_password_access_grant, AZ-3700_share_authorize_content, AZ-3701_share_abuse_limits
**Component**: 06 sharing
**Tracker**: AZ-3703
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c09

## Problem

Share links are the one place where visitors without an account reach stored files. Tenant isolation (R09, AC-59), token secrecy (AC-12) and the timing and cost bounds need one suite that runs against all sharing services together, and performance scenarios that the reference-server run (E-16/c10) can execute.

## Outcome

- ST-06-1, ST-06-2, ST-06-3, ST-06-7 and ST-06-8 pass in CI (env `C`).
- PT-06-1, PT-06-2 and PT-06-3 exist as runnable scenarios; their functional assertions run in CI and their timing assertions report `NOT-RUN(HW-REF)` off the reference server (env `C-REF`).
- ST-06-4 and ST-06-5 (E-16/c05) and ST-06-6 (E-16/c07) are written by their tasks and run in the same suite.

## Scope

### Included
- The five security tests and three performance scenarios exactly as 06 `tests.md` specifies them, with the link factory, password corpus and token corpus of its test data.

### Excluded
- Running the timing scenarios on REF-SRV and recording results (E-16/c10).
- System scenarios NFT-SEC-16, 17 (E-31).

## Acceptance Criteria

**AC-1: No cross-account owner operations (ST-06-1, AC-59)**
Given A's link on file f
When B calls `create(B, f)`, `list(B)`, `list(B, node = f)` and `revoke(B, A's share id)`, and a visitor then opens A's link
Then B gets `TargetNotFound`, empty lists and `NotFound`; A's row is unchanged and the link still opens

**AC-2: Tokens are strong and never stored or logged (ST-06-2)**
Given 10,000 links created through the service
When the tokens are decoded, `share_links` is dumped with the migrator role, and logs and audit rows are captured
Then every token has prefix `sc_shr_` and 256 random bits, all 10,000 are distinct with no common prefix beyond the marker, a chi-square test on the bytes does not reject uniformity at p = 0.001, and 0 plaintext tokens or passwords appear in the database, logs or audit

**AC-3: Guessing yields identical answers and trips the limit (ST-06-3)**
Given 10,000 random and near-miss tokens (one character changed from a live token, truncated, case-changed)
When one IP opens them
Then every answer is `NotFound` until call 101 in the minute and `RateLimited` after; no near-miss answer differs from a random one

**AC-4: Visitors see four fields only (ST-06-7)**
Given every visitor outcome of `open` and `verify_password`
When they are serialised and the `Available` field list is read
Then the fields are exactly name, size, media type and expiry, and no outcome carries an account id, owner, path or node id

**AC-5: Row-level security and the single cross-account lookup (ST-06-8, AC-59)**
Given links of A and B
When the API role scoped to B selects `share_links`, calls the lookup with an exact token hash and with partial input, and the source is searched for scope switches
Then B sees only its rows; the lookup returns one row for the exact hash and nothing for partial input; the lookup is the only scope switch in sharing

**AC-6: Performance scenarios are runnable (PT-06-1..3)**
Given the three scenarios of 06 `tests.md` (1,000 req/s open and content mix for 3 minutes; 10,000 interleaved samples per missing case; 1,000 concurrent wrong passwords for 60 s while 50 visitors open)
When they run off the reference server
Then the functional assertions pass (error rate 0%, response values equal, permits within `share_password_max_concurrency`) and each timing assertion reports `NOT-RUN(HW-REF)`, never PASS

## Non-Functional Requirements

**Performance**
- Targets carried by the scenarios: `open` p95 at most 10 ms (fail 50 ms); `authorize_content` p95 at most 15 ms (fail 60 ms); missing-case median difference at most 0.3 ms (fail 1 ms); `open` p95 during the flood at most 50 ms (fail 200 ms); DB connections at most 12; hashing CPU at most permits x 1 core.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Chi-square helper on a known uniform and a biased sample | Accepts the first, rejects the second |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | A's link on f | ST-06-1 | 0 cross-account successes | NFT-SEC-13 |
| AC-2 | 10,000 links | ST-06-2 | Prefix, 256 bits, distinct, uniform; 0 plaintext | NFT-SEC-16, FT-P-38 |
| AC-3 | Token corpus | ST-06-3 | Identical `NotFound`; limited at 101 | NFT-SEC-17, FT-N-09 |
| AC-4 | All visitor outcomes | ST-06-7 | Four fields only | NFT-SEC-51 |
| AC-5 | Links of A and B | ST-06-8 | Own rows; exact-hash only; one scope switch | NFT-SEC-46 |
| AC-6 | PT-06-1..3 scenarios | Run off REF-SRV | Functional pass; timing `NOT-RUN(HW-REF)` | PT-06-1, PT-06-2, PT-06-3 |

## Constraints

- A timing result from a workstation is never reported as PASS (`tests/environment.md`, code C-REF).
- Real services, real tree and real PostgreSQL 18; no fake store.

## Risks & Mitigation

**Risk 1: Flaky timing on shared CI runners**
- *Risk*: timing assertions fail or pass by chance on CI.
- *Mitigation*: timing assertions only count on REF-SRV (E-16/c10).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The epic row lists ST-06-1..8, but ST-06-4 and ST-06-5 are already on E-16/c05 and ST-06-6 on E-16/c07; this task runs them in the suite but does not re-author them | E-16 owner | resolved | Low |
