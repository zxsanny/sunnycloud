# TokenStore and device login client

**Task**: AZ-3908_token_store_device_login
**Name**: Token store and device login
**Description**: Keep the device token only in the OS keyring or, on Linux without a Secret Service, in a 0600 file inside a 0700 directory with one warning; serve it to sync-core as the `CredentialProvider`; run the RFC 8628 device login (prompt, browser, polling at the server interval with `slow_down`); and move to signed-out on revocation or logout
**Complexity**: 3 points
**Dependencies**: AZ-3885_linux_keyring_systemd_spike, AZ-3891_local_index_store, AZ-3959_http_sync_client_connect, AZ-3966_token_lifecycle, AZ-3967_sync_host_traits_foundation, AZ-3763_sc_config
**Component**: 14 desktop_daemon
**Tracker**: AZ-3908
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `LoginPrompt`, `LoginState`, `LogoutOutcome` (E-24/c05).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CredentialProvider`, `Credential` with `refresh = None` for device tokens (E-23/c14).
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `connect` (`ConnectError`), `device_grant_request`, `device_grant_poll`, `sign_out` (E-23/c07, E-23/c13).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `DeviceAuthorization` (E-23/c04).

## Problem

The desktop signs in without ever seeing a password: the server's device grant shows a user code, the user approves it in a browser, and the daemon receives a sync-only token. That token must never be stored in plain, world-readable places, logs, IPC, process arguments or the environment (AC-48, NFT-SEC-34, RESTRICT-19). Polling must respect the server interval and `slow_down`. A revoked token must stop sync quickly and show signed-out (AC-21).

## Outcome

- `KeyringTokenStore` (keyring 4.2.0 v1 mode, service `sunnycloud`, user = account id) as the primary store on Linux and Windows.
- `UserFileTokenStore` on Linux when no Secret Service answers (detection rule and location from E-24/c03): file 0600 in a 0700 directory, one WARN with the text decided by E-24/c03; a group- or world-readable file is refused with `PermissionTooOpen` and not read; `StoreUnavailable` when neither store works.
- The daemon's `CredentialProvider` reads the token per request through the store.
- Login: `login_start(device name)` calls `device_grant_request`, opens the system browser on the verification URL and returns `LoginPrompt{verification_url, user_code, expires_at}`; polling with `device_grant_poll` at the server interval, plus 5 s after each `slow_down`; `Approved` saves the token through the store only; `Denied` and `Expired` end the attempt; HTTPS required unless the developer build feature is compiled in.
- `Unauthorized` from any sync-core call sets `SignedOut`, stops uploads and feed loops, keeps pairs and the index; an unreachable identity provider alone does not interrupt sync.
- `logout`: `sign_out` (`DELETE /api/v1/devices/me`), then the token is deleted from the store even if the server call failed, and the outcome says the server copy still needs revoking.

## Scope

### Included
- Both stores, the provider, the login state machine, revocation and logout handling.

### Excluded
- The IPC methods' wiring (E-24/c19); server-side grant rules and exact-once minting (E-15/c07); token refresh (desktop device tokens never rotate in v1.0); the end-to-end login against the real server (E-24/p01).

## Acceptance Criteria

**AC-1: Token only in the keyring or a 0600 file**
Given Linux with a Secret Service, Linux without one, and a pre-created 0644 token file
When a token is saved and loaded
Then with a Secret Service there is one keyring entry `sunnycloud/<account id>`; without one the token is in a 0600 file in a 0700 directory at the location decided by E-24/c03 with exactly one WARN line; the 0644 file gives `PermissionTooOpen` and is not read (IT-14-18)

**AC-2: Token nowhere else (NFT-SEC-34)**
Given a signed-in daemon process
When IPC traffic, the log files, `/proc/<pid>/cmdline` and `/proc/<pid>/environ` are searched for the token string
Then it occurs 0 times

**AC-3: Polling honours the interval and `slow_down` (AC-54)**
Given a fake grant server with interval 5 s (config mock), one `slow_down`, then approval
When the login runs
Then every poll gap is at least the interval, gaps after `slow_down` are 5 s longer, `login_start` returned only `{verification_url, user_code, expires_at}`, and the approved token was saved through the store (IT-14-23)

**AC-4: Denied, expired and plain HTTP**
Given a fake grant server that denies, one that expires, and a server address with `http://`
When login runs for each
Then `login_status` is `Denied`, then `Expired`, and the `http://` case fails with `HttpsRequired` before any request

**AC-5: Revocation stops sync (AC-21)**
Given a signed-in daemon whose device is revoked through the API while a feed poll is held, and separately the identity provider made unreachable
When the next call returns `Unauthorized`
Then within 5 s `login_status` is `SignedOut`, uploads stop, pairs and index are kept; the provider outage alone changes nothing (IT-14-21)

**AC-6: Logout with the server down**
Given a signed-in daemon and an unreachable server
When `logout` runs
Then the local token is deleted and the outcome reports that the server copy still needs revoking

## Non-Functional Requirements

**Security**
- No token or user code in logs at any level; the token never crosses the IPC boundary.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | File store modes; 0644 file | 0600/0700; one WARN; `PermissionTooOpen` |
| AC-3 | Poll gaps with interval and `slow_down` on `ManualClock` (IT-14-23) | >= interval; +5 s after `slow_down` |
| AC-4 | Denied, Expired, `http://` | States; `HttpsRequired` |
| AC-6 | `sign_out` failing | Token deleted; outcome says server copy remains |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Linux with and without Secret Service; 0644 file | IT-14-18 | Keyring entry or 0600 file with one WARN; refused 0644; token absent elsewhere | AC-48, FT-P-61, NFT-SEC-34 |
| AC-3, AC-4 | Fake grant server | IT-14-23 | Interval and `slow_down` honoured; states; `HttpsRequired` | AC-54, NFT-SEC-12 |
| AC-5 | Server; device revoked; provider down | IT-14-21 | `SignedOut` within 5 s; pairs kept | AC-21, NFT-SEC-08, FT-N-30 |

## Runtime Completeness

- Named capability: keyring 4.2.0 (Secret Service on Linux, Credential Manager on Windows) and the RFC 8628 device-grant client (14 §2, §5).
- Production code that must exist: real keyring calls and the real file fallback with permission checks.
- Allowed external stubs: a fake grant server in unit and IT-14-23 runs; the real grant is exercised by E-24/p01.
- Unacceptable substitutes: an in-memory token store in the daemon, a token in the LocalIndex or a config file.

## Constraints

- IT tier: the module runs in-process with the real `HttpSyncClient` against the server container (profile `desktop`); the daemon-binary run is E-24/p01.

## Risks & Mitigation

**Risk 1: Keyring blocks without a session bus**
- *Risk*: the call hangs and the token is never stored.
- *Mitigation*: the detection rule and its time bound come from E-24/c03.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `LoginPrompt.expires_at` needs the grant's lifetime; 13's `DeviceAuthorization` has no expiry field | E-23/c13 | open | Low |
| WARN text and file location wait for E-24/c03 (G-12) | E-24 | open | Low |
