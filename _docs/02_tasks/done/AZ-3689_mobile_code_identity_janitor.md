# Mobile one-time code exchange and the IdentityMaintenance janitor

**Task**: AZ-3689_mobile_code_identity_janitor
**Name**: Mobile sign-in code and identity janitor
**Description**: Hand a mobile app its session through a single-use, PKCE-bound, short-lived code instead of a redirect carrying tokens, and remove expired identity rows on a schedule
**Complexity**: 3 points
**Dependencies**: AZ-3684_provider_sign_in_callbacks, AZ-3686_session_refresh_rotation, AZ-3687_device_grant_exact_once, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base; notes: E-31/c13 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 01 identity
**Tracker**: AZ-3689
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c09

## Problem

A mobile app signs in through the system browser, so the callback cannot hand it tokens directly. The callback instead returns a one-time code that the app exchanges, with its PKCE verifier, for a normal session (decision 11). The code must be single use, bound to the app's challenge and short-lived; it is stored because the callback and the exchange may hit different requests or a restarted process. Expired states, codes, grants and old sessions pile up unless a janitor removes them.

## Outcome

- `complete` for a mobile client kind (`android`, `ios`) returns `MobileCode{code, return_to}` and stores a `login_codes` row; `exchange_mobile_code(code, code_verifier)` returns session tokens.
- One `InvalidCode` value for a reused, wrong-verifier or expired code (AC-56 case 12).
- `IdentityMaintenance` implements the `Janitor` port (`name`, `run_cycle(now) -> CycleStats`).

## Scope

### Included
- Mobile branch of `complete`: instead of a session, store `login_codes` (code hash, account, `code_challenge` copied from the state row, client kind, expiry `login_code_ttl` 2 min) and return the code.
- `exchange_mobile_code`: consume the row by delete-returning, check the S256 verifier against the stored challenge and the expiry, then mint a `sessions` row with `client_kind` android or ios, an access token for `access_token_ttl` and a rotating refresh token.
- `IdentityMaintenance::run_cycle(now)`: delete `oauth_states` past expiry, `login_codes` past expiry, `device_grants` 1 h after expiry or consumption, `sessions` 30 d after revocation or refresh expiry; return counts.

### Excluded
- Begin-time validation that a mobile client sends a `code_challenge` (E-15/c04).
- Refresh rotation itself (E-15/c06); the app side (E-27).
- Scheduling the janitor (E-22/c06).

## Acceptance Criteria

**AC-1: Mobile sign-in returns a one-time code**
Given `begin` with client kind `android` and a code challenge, and mock-idp `valid` for a known identity
When `complete` runs
Then it returns `MobileCode{code, return_to}`, no session exists yet, and `login_codes` holds only the code's hash with the challenge copied from the state row

**AC-2: The code exchanges once for a mobile session (AC-19, AC-58)**
Given the code of AC-1 and `login_code_ttl = 500 ms`
When it is exchanged with the right verifier, then exchanged again
Then the first exchange returns session tokens (access TTL from config, rotating refresh) for a `sessions` row with `client_kind = android`; the second returns `InvalidCode` and mints nothing

**AC-3: Wrong verifier and expired code fail the same way (AC-56 case 12)**
Given a fresh code exchanged with a wrong verifier, and another code exchanged after the clock passes `login_code_ttl`
When each exchange runs
Then both return the same `InvalidCode` value as a reused code, the rows are consumed, and no session is minted

**AC-4: The janitor removes only expired rows**
Given fixture rows on both sides of each boundary: `oauth_states` and `login_codes` just before and after expiry, `device_grants` 59 and 61 minutes after expiry or consumption, `sessions` 29 and 31 days after revocation or refresh expiry
When `run_cycle(now)` runs twice
Then the first run deletes exactly the rows past their boundary and reports their counts per table; rows inside their boundary stay; the second run deletes 0

## Non-Functional Requirements

**Reliability**
- The exchange and the session mint are one transaction; a failed exchange leaves no session.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | S256 check with the RFC 7636 test vector | Right verifier passes; any change fails |
| AC-4 | Boundary per table with the manual clock | Deletion exactly past each boundary |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mobile begin with challenge | `complete` | `MobileCode`; no session; hash and challenge stored | |
| AC-2 | Code, TTL 500 ms | IT-01-14 exchange twice | Tokens once, `client_kind = android`; then `InvalidCode` | FT-P-29 |
| AC-3 | Wrong verifier; expired code | IT-01-14, IT-01-5 case 12 | Same `InvalidCode`; no session | FT-N-23, NFT-SEC-03 |
| AC-4 | Rows around each boundary | IT-01-14 janitor part | Only past-boundary rows deleted; second run 0 | |

## Constraints

- Mobile sign-in yields a `sessions` row, never a device token (canonical names: the device grant is desktop only).
- The janitor uses system scope and one short transaction per batch.

## Risks & Mitigation

**Risk 1: Code intercepted on the device**
- *Risk*: another app catches the redirect with the code.
- *Mitigation*: PKCE binds the code to the app's verifier; single use; 2-minute TTL.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Janitor batch size per table is not stated in 01 | E-15 owner | open | Low |
