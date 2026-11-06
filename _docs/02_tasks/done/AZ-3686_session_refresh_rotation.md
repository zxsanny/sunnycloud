# SessionService: refresh rotation, reuse grace, family revocation, list and touch

**Task**: AZ-3686_session_refresh_rotation
**Name**: Session refresh rotation and revocation
**Description**: Rotate refresh tokens so that concurrent refreshes yield one successor, revoke the whole session when an old token is replayed after the grace window, and let users list and revoke their sessions
**Complexity**: 5 points
**Dependencies**: AZ-3683_authenticator_scopes_cache, AZ-3684_provider_sign_in_callbacks, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base
**Component**: 01 identity
**Tracker**: AZ-3686
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/authenticator.md` — revocation must flip `revalidate` within its bound (produced by E-15/c03).

## Problem

Access tokens live 15 minutes, so web and mobile clients refresh often. A mobile client can fire several refreshes at once after waking; they must not revoke the user (R05). A stolen refresh token replayed later must end the whole session. Users and admins need to see and end sessions, and that must reach live requests within 5 s (AC-21).

## Outcome

- `SessionService::refresh` rotates per data model §4.3; four concurrent refreshes give one successor and no revocation (AC-58).
- A replay of the previous token after `refresh_reuse_grace` (30 s) returns `ReuseDetected` and revokes the session (one row is one token family) with reason `refresh_reuse`.
- `revoke`, `revoke_all`, `list` and `touch` behave as below.

## Scope

### Included
- `refresh(refresh_token)`: under the row lock of the session found by `refresh_hash` (not revoked), issue a new access token (`access_token_ttl`) and refresh token, move the old refresh hash to `prev_refresh_hash`, set `rotated_at`. Presenting `prev_refresh_hash` within `refresh_reuse_grace` of `rotated_at` creates nothing and revokes nothing. Presenting it after the grace sets `revoked_at` with reason `refresh_reuse` and returns `ReuseDetected`. An older generation, an unknown token or an expired refresh token is `Unauthenticated`.
- `revoke(account, session_id, reason)`, `revoke_all(account, reason)`: set `revoked_at` and `revoke_reason` on rows of that account only.
- `list(account)`: `SessionInfo` rows of that account (client kind, label, created, last seen, created IP), never a token.
- `touch`: write `last_seen_at` at most once per `session_last_seen_interval` (1 min).

### Excluded
- The audit event and WARN log for refresh reuse and session revocation (E-15/c11).
- Minting the first session at sign-in (E-15/c04, E-15/c09 for mobile).
- CSRF part of AC-58 (E-21/c08); route mapping (E-21/c12).

## Acceptance Criteria

**AC-1: Rotation issues a working pair**
Given one session with refresh token T0
When `refresh(T0)` runs
Then it returns a new access token and refresh token T1, both authenticate as the session's account, and T0 is now the stored previous token

**AC-2: Concurrent refreshes give one successor (AC-58)**
Given one session with refresh token T0 and `refresh_reuse_grace = 300 ms`
When 4 `refresh(T0)` calls run in parallel
Then exactly 1 new refresh token exists for the session, the session is not revoked, and all four calls receive that one successor pair (the old access token stops working at the next rotation, within `session_cache_ttl`: relaxed by the owner on 2026-11-06, because the session keeps one access hash; clients repeat a request that gets a 401)

**AC-3: Replay after the grace revokes the family (AC-58)**
Given the state after AC-2
When the clock advances 400 ms and `refresh(T0)` is called again
Then it returns `ReuseDetected`, the session gets `revoked_at` with reason `refresh_reuse`, and every token of the session fails `authenticate` within 5 s

**AC-4: Dead refresh tokens change nothing**
Given a session rotated twice (T0, T1, T2), an expired refresh token and a random token
When `refresh` is called with T0 (older than the previous generation), the expired token and the random token
Then each returns `Unauthenticated` and no row changes

**AC-5: Revocation reaches live requests (AC-21, IT-01-2 rows)**
Given `session_cache_ttl = 200 ms` and two warm session principals
When one is ended by `revoke` and the other by `revoke_all`
Then `revalidate` turns false for each within 200 ms + 50 ms and `authenticate` then fails

**AC-6: An account sees and ends only its own sessions (AC-59)**
Given accounts A and B with two sessions each
When B calls `list(B)` and `revoke(B, A's session id)`
Then the list holds only B's sessions without tokens, A's session is unchanged and still authenticates

**AC-7: Touch writes at most once a minute**
Given one session and the manual clock
When `touch` is called 10 times within 1 minute, then once after the clock passes 1 minute
Then `last_seen_at` is written once, then once more

## Non-Functional Requirements

**Reliability**
- Rotation takes the row lock of one session only; the refresh path holds no lock across external I/O.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2, AC-3 | Grace decision with the manual clock | Within grace: no change; after: revoke |
| AC-7 | Touch interval decision | Write only when the interval has passed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | One session | Single refresh | New pair works; old moved to previous | |
| AC-2 | Grace 300 ms | IT-01-10: 4 parallel refreshes | One successor; not revoked | NFT-SEC-10 |
| AC-3 | After AC-2, clock +400 ms | IT-01-10: replay | `ReuseDetected`; revoked `refresh_reuse`; tokens fail within 5 s | NFT-SEC-10 |
| AC-4 | Rotated twice; expired; random | `refresh` | `Unauthenticated`; no change | |
| AC-5 | TTL 200 ms | IT-01-2: `revoke`, `revoke_all` paths | Flip within 250 ms | NFT-SEC-08, NFT-RES-26 |
| AC-6 | A and B, 2 sessions each | `list`, foreign `revoke` | Own rows only; A unchanged | ST-01-1 |
| AC-7 | Manual clock | `touch` x10, then +1 min | 1 write, then 1 more | |

## Constraints

- Only one previous generation is kept (`prev_refresh_hash`); older tokens are simply unknown (data model §4.3).
- Revoked and expired are decided in code after the lookup, as in the Authenticator.

## Risks & Mitigation

**Risk 1: Lost rotation on mobile (R05)**
- *Risk*: a client loses the response to a rotation and replays the old token after the grace, signing the user out.
- *Mitigation*: 30 s grace; clients single-flight refresh and persist the new token before use (E-27); AC-2 and AC-3 bound the server side.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| What the 3 losing calls of a concurrent refresh receive is not stated (data model: "creates nothing and revokes nothing"); clients (E-26, E-27) need to know whether they get an error or the successor | E-15 owner with E-27 | open | Medium |
| `revoke` of a foreign or unknown session id: 01 lists only `Unauthenticated`; ST-01-1 accepts `NotFound` or "no row changed" | E-15 owner | open | Low |
