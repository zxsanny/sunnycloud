# DeviceGrantService: device grant with approval view, GeoIP lookup and exactly-once token mint

**Task**: AZ-3687_device_grant_exact_once
**Name**: Desktop device grant
**Description**: Let a desktop daemon obtain a sync-only device token through an RFC 8628 grant approved from a signed-in browser that sees requester against approver location, with the token minted exactly once and never after expiry
**Complexity**: 5 points
**Dependencies**: AZ-3682_identity_crate_migration_config, AZ-3683_authenticator_scopes_cache, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base; notes: E-31/c22 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 01 identity
**Tracker**: AZ-3687
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/authenticator.md` — device principals, scope refusal and revocation latency (produced by E-15/c03).

## Problem

The desktop daemon has no browser of its own and Apple has no device flow, so the server runs its own device grant (ADR-008). Device codes are a phishing target: the approver must see where the request came from (AC-57). A device code must yield a token exactly once and never after it expired, even when polls race (AC-76, interaction risk R9).

## Outcome

- `DeviceGrantService` with `request`, `poll`, `lookup_for_approval`, `decide`, `list_devices`, `revoke_device`, `revoke_self`.
- The token mint is one transaction: the grant moves `approved` to `consumed` by a single conditional update that also requires `expires_at > now`, and the device token row is inserted with it.
- A production `GeoLookup` reads the local GeoIP file at `geoip_db_path`; a missing file means "location unknown".

## Scope

### Included
- `request(device name, platform, requester IP)`: 256-bit device code (only its hash stored), short user code (unique while pending), requester location, expiry `device_code_ttl` (at most 10 min), poll interval `device_poll_interval` (5 s).
- `poll(device_code)`: `AuthorizationPending` before a decision; `SlowDown` when polled faster than the interval (`last_polled_at`); `AccessDenied` after deny; `ExpiredToken` after expiry; the device token once.
- `lookup_for_approval(session principal, user_code, approver IP)` → `ApprovalView{grant_id, expires_at, device, requester_ip, requester_location, approver_ip, approver_location, mismatch}` (the grant id `decide` takes and the code's expiry, owner decision 2026-11-05); unknown or expired code → `UnknownCode`.
- `decide(session principal, grant id, Approve | Deny)` → `UnknownCode`, `Expired`, `AlreadyDecided`.
- `list_devices(account)`, `revoke_device(account, device id)` (`NotFound` for foreign or unknown), `revoke_self(device principal)` (revokes only its own row; a session principal is refused with `Forbidden`).
- Production `GeoLookup` over the local GeoIP file.

### Excluded
- Wrong-code counters and `RateLimited` (E-15/c11); the `device_approved` audit event (E-15/c11).
- Routes and RFC 8628 wire error codes (E-21/c12); the approval page (E-26/c14); the daemon side (E-24, pipeline AZ-3668).
- Janitor deletion of old grants (E-15/c09).

## Acceptance Criteria

**AC-1: A device authorization is issued safely**
Given a daemon request with name `lab-1`, platform linux and requester IP 10.0.0.5
When `request` runs
Then it returns a device code, a user code, the interval and `expires_in` of at most 600 s; only the device code's hash is stored, with the requester IP and location

**AC-2: Pending, too fast and denied grants give nothing (FT-N-45)**
Given a fresh grant
When it is polled before any decision, polled faster than the interval, then denied and polled at the interval, and finally `decide(Approve)` is attempted on it
Then the polls return `AuthorizationPending`, `SlowDown` on each early poll, then `AccessDenied`; the late approval returns `AlreadyDecided`; 0 device tokens exist for the grant

**AC-76: A device code exchanges for a token exactly once, never after expiry (R9)**
Given an approved device grant
When the device code is polled twice concurrently, polled again after the token was issued, and polled for a second grant whose code expired (injectable clock) before approval
Then exactly one poll returns a token, every later poll of that code fails without minting, and the expired code never mints a token

**AC-3: Expiry is enforced on approval and lookup (AC-57)**
Given `device_code_ttl` (test value) and the manual clock
When one grant is approved shortly before expiry, another after expiry, and a lookup and two decisions run on the expired one
Then the first approval succeeds; for the expired grant `lookup_for_approval` returns `UnknownCode`, the first `decide` returns `Expired` and the second `AlreadyDecided`; a grant approved before expiry but polled after it returns `ExpiredToken` with 0 tokens minted

**AC-4: The approval view shows both sides (AC-57)**
Given requester IP 203.0.113.7 and approver IP 198.51.100.9 in two different cities of the GeoIP fixture, the same IP on both sides, and a GeoLookup that knows neither address
When `lookup_for_approval` runs for each
Then different cities give `mismatch = true` with both locations; the same IP gives `mismatch = false`; unknown locations with different IPs give "location unknown" and `mismatch = true`; every view carries the grant's id and its `expires_at`, equal to the stored grant

**AC-5: Device token scope and revocation (AC-21, AC-57)**
Given a minted device token and `session_cache_ttl = 200 ms`
When it is authenticated, checked against `SessionOnly`, then revoked once through `revoke_device` and once (a second device) through `revoke_self`; and a session principal calls `revoke_self`
Then it authenticates as kind Device, `SessionOnly` is `Forbidden`, each revocation turns `revalidate` false within 250 ms and revokes only that device's row, and the session principal gets `Forbidden`

**AC-6: Devices are listed and revoked per account (AC-59)**
Given accounts A and B with two devices each
When B lists devices and calls `revoke_device` with A's device id and with a random id
Then B sees only B's devices; both calls return `NotFound`; A's devices still authenticate

**AC-7: Production GeoIP reader**
Given the DS-32 GeoIP test file at `geoip_db_path`, and separately no file at that path
When the service starts and resolves the two fixture addresses
Then the production reader returns their two cities; without the file the service starts and every location is "unknown"

## Non-Functional Requirements

**Reliability**
- The mint transaction holds only the grant row and the new token row; time comes from the injectable clock (NFT-SEC-49 step 3 compares `device_tokens.created_at` with the grant's `expires_at`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Interval decision with the manual clock | Earlier than the interval gives `SlowDown` |
| AC-4 | Mismatch rule | True when IPs differ or a location differs; false for the same IP |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Request from 10.0.0.5 | `request` | Codes, interval, `expires_in` <= 600 s; hash only | FT-P-66 |
| AC-2 | Fresh grant | IT-01-11 deny and too-fast paths | Pending, SlowDown, AccessDenied, AlreadyDecided; 0 tokens | FT-N-45 |
| AC-76 | approved grant; concurrent polls; repeat poll; expired grant | exchange outcomes | one token only; repeats and expired code mint nothing | NFT-SEC-49, FT-N-45 |
| AC-76 | Approved grant, 20 parallel polls (IT-01-11) | Parallel and sequential polls | Exactly 1 token; `device_tokens` +1; grant consumed | NFT-SEC-49 |
| AC-3 | Manual clock around `device_code_ttl` | IT-01-11 expiry case, IT-01-12 expiry part | Approve before ok; after: UnknownCode, Expired, AlreadyDecided; ExpiredToken, 0 tokens | NFT-SEC-07 |
| AC-4 | DS-32 city pairs; same IP; unknown | IT-01-12 | Mismatch flags as stated; grant id and expiry returned | FT-P-66, FT-P-81, NFT-SEC-07 |
| AC-5 | Minted token; TTL 200 ms | IT-01-2 `revoke_device` path; IT-01-15 `revoke_self` part | Kind Device; Forbidden on SessionOnly; flip within 250 ms; session refused | FT-N-24, NFT-SEC-07 |
| AC-6 | A and B, 2 devices each | Foreign list and revoke | Own rows; `NotFound`; A unaffected | ST-01-1 |
| AC-7 | DS-32 file; no file | Production reader | Two cities; unknown without file | |

## Runtime Completeness

- **Named capability**: GeoIP location from a local database file behind the `GeoLookup` trait.
- **Production code that must exist**: a `GeoLookup` that reads the file at `geoip_db_path` and resolves IPv4 and IPv6 addresses to a coarse location.
- **Allowed external stubs**: the fixed-table `GeoLookup` named in 01 `tests.md` for the city-pair cases of IT-01-12.
- **Unacceptable substitutes**: a production `GeoLookup` that always answers "unknown", hardcoded locations, or the fixed table compiled into the release build.

## Constraints

- Mint: `UPDATE ... SET status = 'consumed' WHERE status = 'approved' AND expires_at > $now RETURNING` plus the `device_tokens` insert in one transaction (01 DeviceGrantService).
- Device tokens are 256-bit with a type prefix, only SHA-256 stored, sync scope only, no rotation in v1.0.
- The end-to-end device login through the daemon and the real server is the pipeline task AZ-3668 (E-24/p01); it owns no interaction risk, so AC-76 lives here.

## Risks & Mitigation

**Risk 1: Phishing with a forwarded user code**
- *Risk*: an attacker sends a victim a user code from the attacker's daemon.
- *Mitigation*: AC-4 mismatch warning; wrong-code limits (E-15/c11); web page defaults to deny on mismatch (FT-P-81, E-26/c14).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The GeoIP file format and reader library are not named in 01 (DS-32 must be built in the same format) | E-15 owner with E-31/c22 | open | Low |
| IT-01-15 lists `revoke_self` and `list_devices` under `AccountAdmin`; they belong to this service and are tested here | E-15 owner | resolved | Low |
| Resolved by owner decision 2026-11-05: `lookup_for_approval` returns the grant id and the code's expiry (`ApprovalView.grant_id`, `expires_at`), so the approval page can call `decide` and show the countdown | Owner | resolved | High |
