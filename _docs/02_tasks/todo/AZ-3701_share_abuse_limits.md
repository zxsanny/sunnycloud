# Share abuse counters with the shared limiter and trip auditing

**Task**: AZ-3701_share_abuse_limits
**Name**: Share abuse limits
**Description**: Limit invalid share tokens per IP and wrong share passwords per link on the shared abuse limiter, so guessing and hash-cost floods get RateLimited with a retry time and cannot be bypassed by spreading across IPs
**Complexity**: 3 points
**Dependencies**: AZ-3695_sharing_gaps_grant_key_spike, AZ-3698_share_lookup_open_uniform, AZ-3699_share_password_access_grant, AZ-3700_share_authorize_content, AZ-3784_sc_audit, AZ-3785_sc_ratelimit
**Component**: 06 sharing
**Tracker**: AZ-3701
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c07

## Problem

Share tokens and passwords are guessable targets. AC-20 sets 100 invalid share tokens per minute per IP and 10 wrong share passwords per link per 10 minutes, with HTTP 429 and Retry-After beyond them. The password limit must hold per link, whatever the number of attacking IPs (ST-06-6), so at most 10 Argon2 verifications run per link per window.

## Outcome

- `open` and `authorize_content` count unknown tokens per IP; `verify_password` counts wrong passwords per link; past a limit they return `RateLimited{retry_after}`.
- One audit event and one WARN log per trip, with the link id or an IP hash, never the token.

## Scope

### Included
- Per-IP invalid-token counter (`share_invalid_token_limit`, 100 per minute) on `open` and `authorize_content` (the content route shares the counter, 06 §3).
- Per-link wrong-password counter (`share_wrong_password_limit` 10 per `share_wrong_password_window` 10 min) on `verify_password`, checked before any Argon2 work.
- A correct password while the link is limited follows E-16/c01's G-06-2 decision.
- Trip audit and WARN log.

### Excluded
- The limiter (E-08/c01); 429 mapping (E-21/c09); token normalisation of the path (11, before 06 sees the token).

## Acceptance Criteria

**AC-1: Invalid tokens are limited per IP (AC-20)**
Given windows scaled to 2 s
When IP1 opens 100 unknown tokens and then a 101st, IP2 opens unknown tokens meanwhile, and the window passes
Then IP1's first 100 calls give `NotFound` and the 101st `RateLimited` with `retry_after > 0`; IP2 is not limited; after the window IP1 is allowed again

**AC-2: Wrong passwords are limited per link (AC-20)**
Given password links L1 and L2
When IP1 sends 10 wrong passwords to L1 and then an 11th, IP2 then sends one to L1, and IP1 sends one to L2
Then the 11th call and IP2's call on L1 give `RateLimited{retry_after}`; L2 still answers `WrongPassword`

**AC-3: A correct password while limited follows G-06-2**
Given L1 limited by AC-2
When the right password is sent before the window ends, and again after it
Then the first answer is the outcome recorded by E-16/c01 (the tests expect `RateLimited`), and after the window the right password returns a grant

**AC-4: The limit cannot be bypassed (ST-06-6)**
Given one link and 50 parallel wrong attempts from 50 IPs released at a barrier, plus the same link reached through token forms that 11 normalises to the same token
When the attempts run
Then at most 10 Argon2 verifications execute in the window and the rest get `RateLimited`

**AC-5: One audit event per trip**
Given the trips of AC-1 and AC-2
When the audit log and WARN logs are read
Then each trip wrote exactly one audit event and one WARN line, with the link id or an IP hash and never the token

## Non-Functional Requirements

**Reliability**
- Counters live in memory; a restart resets them (06 §5). A fresh limiter per test keeps counters from leaking between tests.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Order of checks in `verify_password` | Limit checked before any Argon2 work |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 2 s windows; IP1, IP2 | IT-06-8 invalid tokens | 101st limited; IP2 free; reset | NFT-SEC-04, ST-06-3 |
| AC-2 | L1, L2 | IT-06-8 wrong passwords | 11th limited per link; L2 free | NFT-SEC-06, FT-N-10 |
| AC-3 | L1 limited | IT-06-8 correct password | Per G-06-2; grant after window | |
| AC-4 | 50 IPs at a barrier | ST-06-6 | At most 10 verifications | NFT-SEC-06 |
| AC-5 | Trips | IT-06-8 audit and log | One event, one WARN each; no token | NFT-SEC-16 |

## Constraints

- One `AbuseLimiter` for the whole server, built by 12; 06 never constructs one.

## Risks & Mitigation

**Risk 1: Shared IPs (NAT) hit the invalid-token limit**
- *Risk*: many visitors behind one address trip the per-IP counter.
- *Mitigation*: only invalid tokens count; valid opens are not limited by this counter.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Whether `verify_password` with an unknown token also counts against the per-IP invalid-token counter is not stated (06 §3 names the content route only) | E-16 owner | open | Low |
