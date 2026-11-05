# Identity abuse counters with the shared limiter, and identity audit events

**Task**: AZ-3691_identity_abuse_limits_audit
**Name**: Identity abuse limits and audit events
**Description**: Count failed sign-in steps and wrong device-approval codes on the shared abuse limiter so excess attempts get RateLimited with a retry time, and write the identity audit events with ids only
**Complexity**: 2 points
**Dependencies**: AZ-3684_provider_sign_in_callbacks, AZ-3685_invite_redemption_account_creation, AZ-3686_session_refresh_rotation, AZ-3687_device_grant_exact_once, AZ-3784_sc_audit, AZ-3785_sc_ratelimit
**Component**: 01 identity
**Tracker**: AZ-3691
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c11

## Problem

Sign-in and device approval invite guessing: bad OAuth state or codes, and user-code guessing against a victim's pending grant (NFT-SEC-05). AC-20 sets limits per IP, per approving session and per account. Operators also need an audit trail of sign-ins, approvals, revocations and refresh reuse, without tokens, codes or emails in it.

## Outcome

- Identity counts its failures on the single `AbuseLimiter` (helper E-08, constructed once by 12) and returns `RateLimited{retry_after}` past each limit.
- Audit events `sign_in`, `device_approved`, `session_revoked`, `refresh_reuse` and one event per limiter trip, with ids only, through `AuditWriter` (helper E-07).
- WARN logs for refresh reuse and limiter trips with ids only.

## Scope

### Included
- Failed sign-in steps (bad state, bad provider code, other `begin`/`complete` failures): 10 per minute per IP.
- Wrong user codes in `lookup_for_approval`: 5 per approving session, 20 per account per hour, 50 per IP per hour; while a dimension is limited, even a correct code is `RateLimited` for that caller.
- `RateLimited{retry_after}` from `begin`, `complete` and `lookup_for_approval`.
- Audit events at: successful sign-in (E-15/c04, c05, c09), device approval (E-15/c07), session revocation (E-15/c06, c08, c10), refresh reuse (E-15/c06); one event per limiter trip, not per rejected call.
- Audit write failure handling: logged at ERROR; the request outcome is unchanged unless the event is marked mandatory (01 §5).

### Excluded
- The limiter itself and its windows (E-08/c01); the audit log table and writer (E-07/c01).
- 429 mapping and `Retry-After` header (E-21/c09); per-IP HTTP-level scenarios (NFT-SEC-04, NFT-SEC-05 at system level).
- Share limits (E-16/c07).

## Acceptance Criteria

**AC-1: Device-approval limits trip at 5, 20 and 50 (AC-20)**
Given the real `AbuseLimiter` with windows scaled to milliseconds
When one approving session submits 6 wrong user codes; sessions of one account submit 21 across sessions; sessions of several accounts from one IP submit 51
Then attempts up to each limit return `UnknownCode` and the next returns `RateLimited` with `retry_after > 0`, for each of the three dimensions

**AC-2: Limits are per caller, and a limited caller cannot use a correct code**
Given the state after the first trip of AC-1
When the limited session submits a correct user code, and a session of a different account from a different IP submits the same correct code
Then the limited session gets `RateLimited`; the other session reaches the approval view

**AC-3: Failed sign-in steps per IP (AC-20)**
Given one client IP
When 11 sign-in steps fail within one minute (scaled window), and then a step from another IP runs
Then the 11th returns `RateLimited{retry_after}`; the other IP is not limited; after the window the first IP is allowed again

**AC-4: One audit event per trip**
Given the trips of AC-1 and AC-3
When the audit log is read
Then each trip wrote exactly one event, however many calls were rejected, and the WARN log for each trip carries ids only

**AC-5: Identity audit events carry ids only**
Given one successful sign-in, one device approval, one session revoked by the user and one refresh reuse after the grace (IT-01-10)
When the audit log and logs are read
Then there is one `sign_in`, one `device_approved`, one `session_revoked` and one `refresh_reuse` event, plus one WARN log for the reuse; no token, code, state or email appears in any event detail or log line

**AC-6: A failing audit writer does not change a non-mandatory outcome**
Given an `AuditWriter` decorator that fails every write
When a session is revoked and a device is approved
Then both calls return their normal result and an ERROR is logged for each failed write

## Non-Functional Requirements

**Reliability**
- Counters live in memory; a restart resets them, and a trip is audited once (01 §5).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Event detail builders | Only ids and enums; no free-text secret field exists |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Real limiter, ms windows | IT-01-13 three dimensions | N-th `UnknownCode`, (N+1)-th `RateLimited` | NFT-SEC-05 |
| AC-2 | After first trip | Correct code: limited session; other account | `RateLimited`; other approves | NFT-SEC-05 |
| AC-3 | One IP | IT-01-13 sign-in counter | 11th `RateLimited`; other IP ok; reset | NFT-SEC-04 |
| AC-4 | Trips of AC-1, AC-3 | Audit read | One event per trip | |
| AC-5 | Four events | IT-01-10 audit part; log capture | One of each; ids only | NFT-SEC-38, ST-01-5 |
| AC-6 | Failing writer | Revoke, approve | Normal results; ERROR logged | |

## Constraints

- One `AbuseLimiter` instance for the whole server, built by 12 (AZ-3664 AC-6); identity never constructs one.
- Audit details carry ids and codes of outcomes only (01 §7, §10).

## Risks & Mitigation

**Risk 1: Counter memory under a flood**
- *Risk*: many IPs grow the counter map.
- *Mitigation*: the limiter's bucket eviction (E-08/c01).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 01 lists an "invalid tokens" counter for identity but gives no number (AC-20's 100 per minute per IP is for share tokens); this task counts only the dimensions with numbers until the owner sets one | E-15 owner | open | Medium |
| Which sign-in audit events are "mandatory" (fail the request closed when the audit write fails) is not stated in 01 §5 | E-15 owner | open | Medium |
| AC-20 calls the sign-in limit "failed sign-in steps"; the exact set of failures that count (for example `InvitationRequired`) is not listed | E-15 owner | open | Low |
