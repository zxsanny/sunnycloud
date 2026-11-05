# Desktop device login pipeline and daemon runtime root (F3)

**Task**: AZ-3668_pipeline_desktop_device_login
**Name**: Desktop device login pipeline
**Description**: The daemon's runtime root and its device-code login drive IPC, sync-core, the server's device grant and the token store end to end, giving a signed-in daemon against the real server
**Complexity**: 5 points
**Dependencies**: AZ-3763_sc_config, AZ-3885_linux_keyring_systemd_spike, AZ-3889_ipc_schema_crate, AZ-3891_local_index_store, AZ-3908_token_store_device_login, AZ-3914_ipc_server_endpoint, AZ-3915_tray_autostart, AZ-3917_daemon_control_impl, AZ-3959_http_sync_client_connect, AZ-3966_token_lifecycle, AZ-3967_sync_host_traits_foundation, AZ-3687_device_grant_exact_once, AZ-3683_authenticator_scopes_cache, AZ-3828_identity_admin_handlers, AZ-3944_device_approval_page, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3652_initial_structure; notes: E-31/c16 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon (daemon binary, runtime root)
**Tracker**: AZ-3668
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/p01

## Problem

E-24 plans every daemon part (index, platform layer, watcher, feed loop, token store, IPC server, control surface) but no child composes them into a running process: one tokio runtime, the config, the `LocalIndex`, the token store behind sync-core's `CredentialProvider`, the `HttpSyncClient`, the IPC server and an optional tray. The device login (F3) is the first flow that needs that process, and it crosses the daemon, sync-core (13), the server's device grant (01 via 11) and the web approval page. No planned task drives it end to end.

## Outcome

- The daemon binary starts as a composed process (no tray required) and signs in through the device grant of the real server, storing the device token only in the OS keychain or the 0600 file fallback.
- Every component operation of flow F3 has a production caller in the daemon or the server.
- At least one integration test runs F3 with the real daemon binary against the real server stack.

## Scope

### Included
- Daemon runtime root: one tokio runtime, typed `desktop` config, `LocalIndexStore` open, `TokenStore` (keyring, 0600 file fallback with one WARN), `HttpSyncClient::connect` with the daemon's `CredentialProvider` (server-info check; 426 shows the upgrade message), IPC server with `DaemonControl`, tray only when a session bus exists. It is also the place where E-24/p02 and E-24/p03 spawn pair workers and feed loops.
- Flow order and seams (`system-flows.md` F3 steps 1-6):
  1. IPC `set_server` → `ServerUrl` check and server-info probe.
  2. IPC `login_start(device name)` → `SyncApi::device_grant_request` → 11 → `DeviceGrantService` → `LoginPrompt{verification_url, user_code, expires_at}` (never a token).
  3. Daemon polls `SyncApi::device_grant_poll` at the interval; `slow_down` honoured.
  4. A signed-in browser session approves (as the approval page E-26/c14 does) → next poll returns the token once → `TokenStore::save` → `login_status = Approved`.
  5. First authenticated call (root listing or head cursor) with the device token.
  6. Device revoked in the web → next call 401 → `SignedOut`; IPC `logout` → `SyncApi::sign_out` then `TokenStore::delete`.
- The end-to-end integration test of the steps above.

### Excluded
- Sync of files (E-24/p02, p03), the settings window (E-25), macOS (v1.1), the updater (E-24/c18), server-side device grant internals (E-15/c07).

## Acceptance Criteria

AC-1..AC-5 are local to this task. No interaction risk is owned here: R9 (AC-76, one exchange per device code) lies inside 01 and is listed for E-15/c07 under "Pending Step 2" in `interaction-risks.md`.

### Wiring and flow criteria

**AC-1: The daemon starts as one composed process**
Given a Linux desktop container with no tray and no session keyring
When the daemon binary starts with a server address
Then it opens its index, serves the IPC endpoint, checks the server info, and reports status over IPC without a tray

**AC-2: Device login end to end**
Given the daemon and a user signed in on the web of the real server
When IPC `login_start` runs, the user approves the shown user code, and the daemon keeps polling
Then `login_start` returns a verification URL and user code but no token, `login_status` becomes `Approved`, the token is stored in the 0600 file with one WARN (no keyring), and the daemon's first authenticated call succeeds

**AC-3: Denied, expired and too-fast polling**
Given three login attempts
When the user denies one, lets one expire (test clock past 10 minutes) and the server answers `slow_down` to the third
Then `login_status` reports `Denied` and `Expired` for the first two, and the daemon widens its poll interval for the third

**AC-4: Scope, revocation and logout**
Given a signed-in daemon
When its token calls a session-only route, the device is revoked in the web, and in a second run IPC `logout` is called with the server unreachable
Then the session-only call gets 403, after revocation the daemon reports `SignedOut` and asks to sign in, and `logout` deletes the local token even though the server call failed

**AC-5: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `DaemonControl::{set_server, login_start, login_status, logout}` (IPC server), `HttpSyncClient::connect`, `SyncApi::{device_grant_request, device_grant_poll, sign_out}` (daemon), `TokenStore::{save, load, delete}` behind the `CredentialProvider`, and no IPC method that returns a token (schema walk)

## Non-Functional Requirements

**Performance**
- Daemon start to IPC ready within 2 s on LIN-VM.

**Compatibility**
- Linux in CI (desktop-linux container); Windows on WIN-VM (lab, reported NOT-RUN when absent).

**Reliability**
- The token never appears in logs, IPC messages or the index.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Static caller check and `ipc_schema` walk | Each named call has a production caller; no token-returning method |

## Blackbox Tests

Integration through the real daemon binary and the real server stack; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Linux container, no tray, no keyring | Start daemon | IPC ready, status works | AC-26 |
| AC-2 | Signed-in web user | `login_start`, approve, poll | URL + code only; Approved; 0600 file + WARN; call works | AC-48, AC-54 |
| AC-3 | Three logins, test clock | Deny, expire, slow_down | Denied, Expired, interval widened | |
| AC-4 | Signed-in daemon | Session-only call; revoke; logout offline | 403; SignedOut; token deleted | AC-57, AC-21 |

## Constraints

- Owner: component 14 (`src/desktop/daemon/`): the runtime root (`main` composition) and this flow's end-to-end tests among the daemon's integration tests. Server behaviour comes from the real stack; a missing server behaviour goes back to its E-15 or E-21 task.
- No fake of sync-core or of the server: the daemon talks to the real `serve` behind Caddy in the `desktop` profile.
- Downstream: E-31/c29 (FT-P-61, FT-P-66) and E-31/c37 should depend on this task.

## Risks & Mitigation

**Risk 1: Keyring blocks in a headless session**
- *Risk*: the keyring call hangs without a session bus, so the daemon never stores the token.
- *Mitigation*: E-24/c03 spike; AC-2 runs headless and expects the file fallback.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-24 has no child that composes the daemon process; this task owns the runtime root | This task | resolved | Medium |
| The approval page is exercised through HTTP with a session here; the page itself is tested by E-26/c14 against the real server | E-26 | accepted-risk | Low |
