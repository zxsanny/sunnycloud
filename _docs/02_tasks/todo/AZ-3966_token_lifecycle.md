# Token lifecycle: single-flight refresh, on_rotated, device grant, mobile code, sign_out

**Task**: AZ-3966_token_lifecycle
**Name**: Token lifecycle
**Description**: Refresh a mobile session single-flight on a 401 with the new refresh token persisted before use, stop at once for desktop device tokens, and provide the device-grant, mobile-code exchange and sign-out calls
**Complexity**: 5 points
**Dependencies**: AZ-3958_sync_typed_errors, AZ-3959_http_sync_client_connect, AZ-3967_sync_host_traits_foundation, AZ-3970_scripted_fake_server, AZ-3765_sc_clock
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3966
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `refresh`, `device_grant_request`, `device_grant_poll`, `exchange_mobile_code`, `sign_out` (this task contributes them).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CredentialProvider`, `Credential`, `on_rotated` error (E-23/c14).
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `ManualClock` for the time-compressed fixture (E-03/c01).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — auth and device routes (draft).

## Problem

Mobile access tokens live 15 minutes and refresh tokens rotate (ADR-008). Four requests that hit a 401 at the same moment must cause one refresh, not four: concurrent refreshes with one token must yield one successor (AC-58), and a lost new refresh token signs the user out (R05). The new refresh token must be stored by the host before the new access token is used. A desktop device token has no refresh token: a 401 means revoked, and work must stop at once (AC-21). Long uploads and the feed loop must survive an expiry in the middle of a transfer. None of this exists in the client yet.

## Outcome

- One refresh in flight per `HttpSyncClient`; concurrent callers wait for it and retry their request once.
- Order fixed: `on_rotated(access, refresh)` returns `Ok` before any request uses the new access token; a failed persist is retried once, then treated as `Unauthorized` and all work stops (Step 5 gap default).
- `device_grant_request`, `device_grant_poll`, `exchange_mobile_code`, `refresh` and `sign_out` with the typed results of the contract.

## Scope

### Included
- 401 handling in the request path: refresh-capable credential → single-flight `refresh` and one retry of the original call (block PUT, poll, any other call); `refresh = None` → `Unauthorized` at once, no refresh.
- `refresh()`: transport failure retried with the same refresh token (the server's 30 s reuse grace covers a lost response); `Unauthorized` (revoked, expired, reuse detected) stops all work and is reported.
- Persist step and its single retry; no request carries the new access token before it succeeded.
- `device_grant_request(device name, platform)` → `DeviceAuthorization`; `device_grant_poll(device_code)` → `Token | Pending | SlowDown | Denied | Expired`.
- `exchange_mobile_code(code, code_verifier)` → `SessionTokens` or `InvalidCode`.
- `sign_out()` → the revoke call of the contract; the host clears its copy afterwards.
- Sentinel-token checks over every failure path of this task.

### Excluded
- Token storage (hosts: 14 token store, 17 `TokenVault`, 18 Keychain).
- The poll loop timing of the device grant (interval, slow-down) and the sign-in UI (14, 17, 18; pipelines AZ-3668, AZ-3672).
- Server rules: rotation, reuse grace, exact-once device mint (E-15/c06, c07, c09).

## Acceptance Criteria

**AC-1: Four concurrent 401s cause one refresh (AC-58)**
Given a fake server with 15-minute access tokens on a test clock and rotating refresh tokens (NFT-RES-33 fixture), and 4 requests in flight when the access token expires
When all 4 get a 401
Then the refresh endpoint is called exactly once and all 4 requests are retried once and succeed

**AC-2: Persist before use (R05)**
Given a `CredentialProvider` spy whose `on_rotated` sleeps 300 ms
When a refresh succeeds
Then `on_rotated` is called once with the new access and refresh token, and no request carries the new access token before `on_rotated` returned

**AC-3: Persist failure stops work**
Given `on_rotated` fails twice in a row
When a refresh succeeds on the server
Then `on_rotated` is called twice (one retry), the operation returns `Unauthorized`, and the client sends no further request

**AC-4: Transport failure keeps the same refresh token**
Given the refresh request fails on transport once
When the client retries the refresh
Then the retry carries the same refresh token and succeeds inside the server's 30 s grace

**AC-5: Revoked session stops all work (AC-21)**
Given the refresh endpoint answers 401 (revoked)
When the in-flight operations are waiting for the refresh
Then every one of them ends with `Unauthorized` and the client sends no further request

**AC-6: Device token never refreshes (AC-21)**
Given `Credential{access, refresh: None}` and a poll answered 401
When the poll returns
Then the result is `Unauthorized` within 1 s, the refresh endpoint receives 0 requests and `on_rotated` is never called

**AC-7: Mobile code and device grant answers (AC-76 client mapping)**
Given a fake answering a reused mobile code as invalid, and a device grant whose polls answer pending, slow down, denied, expired and finally a token
When `exchange_mobile_code` and `device_grant_poll` are called
Then the results are `InvalidCode`, and `Pending`, `SlowDown`, `Denied`, `Expired` and `Token` in that order

**AC-8: A 20-minute transfer survives expiry (NFT-RES-33)**
Given the 15-minute fixture with time compressed through the test clock
When an upload that spans 20 minutes of test time runs
Then the 401 of the block PUT after expiry is refreshed and the PUT repeated, and the upload completes with no lost block

**AC-9: Tokens never leak on these paths**
Given sentinel tokens `ACC-SENTINEL` and `REF-SENTINEL` and DEBUG logging
When every failure path of AC-1..AC-8 runs
Then logs, `Display` and `Debug` of every error, and every request URL contain 0 occurrences of either sentinel

## Non-Functional Requirements

**Reliability**
- Single-flight per `HttpSyncClient`; refresh never runs twice at once for one client.

**Security**
- Tokens travel only in the `Authorization` header or the refresh body, never in a URL.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Single-flight gate with 4 concurrent waiters | One refresh; four retries |
| AC-3 | Persist failure handling | One retry, then `Unauthorized` |
| AC-6 | Credential without refresh token | No refresh attempted |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Scripted fake (E-23/c17), `ManualClock` | 4 concurrent 401s | 1 refresh; 4 successes | IT-13-9 (a), NFT-SEC-10 |
| AC-2 | Spy with 300 ms `on_rotated` | Order of use | No early use | IT-13-9 (b), NFT-RES-33 |
| AC-3 | Failing `on_rotated` | Persist failure | `Unauthorized`; no more requests | IT-13-9 (BLOCKED sub-case, resolved by the gap default) |
| AC-4 | Fake transport failure on refresh | Retry | Same token; accepted | IT-13-9 (c) |
| AC-5 | Fake revokes | In-flight ops | `Unauthorized`; silence | IT-13-9 (d), NFT-RES-26 |
| AC-6 | Device credential | 401 on poll | `Unauthorized` in 1 s; 0 refreshes | IT-13-10, NFT-SEC-08 |
| AC-7 | Fake code and grant answers | Typed results | As listed | IT-13-9 (e) |
| AC-8 | Time-compressed fixture | 20-minute upload | Completes | NFT-RES-33 |
| AC-9 | Sentinel tokens | All failure paths | 0 occurrences | ST-13-2 |

## Constraints

- Desktop device tokens never rotate in v1.0 (ADR-008).
- Test time moves through the clock helper; no test sleeps 15 minutes.

## Risks & Mitigation

**Risk 1: Kill between server rotation and host persist**
- *Risk*: the old refresh token is still stored when the process dies.
- *Mitigation*: the server's 30 s reuse grace heals a restart inside it; after that the family is revoked and the host shows sign-in (accepted risk in 17 §15).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `sign_out` uses `DELETE /api/v1/devices/me`, declared `DeviceOnly` in `api_v1_surface.md`: a mobile session would get 403; the mobile sign-out route is not decided (`sync_api.md` open point 4) | E-15 / E-21 / E-27 owners | open | Medium |
| `InvalidCode` and the RFC 8628 poll answers have no `sc_error` rows or body shape yet (`sync_api.md` open point 3) | E-15 / E-02 | open | Low |
| AC-3's behaviour is the Step 5 gap default (retry once, then sign out), not an owner decision | Owner | accepted-risk | Low |
