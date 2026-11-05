# DaemonControl implementation: status, subscribe, server, pairs and settings echo

**Task**: AZ-3917_daemon_control_impl
**Name**: DaemonControl implementation
**Description**: The daemon's implementation of every `DaemonControl` method behind the IPC server and the tray: the status snapshot and its change events, server address with the server-info probe, server-folder listing, pair add, update, remove, pause and resume with their validations, skipped and escaped listings, hold resolution and login delegation, and settings applied at once and echoed
**Complexity**: 5 points
**Dependencies**: AZ-3889_ipc_schema_crate, AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3894_watcher_scanner_settler_rules, AZ-3903_delete_guard_holds, AZ-3906_first_sync_merger, AZ-3907_placeholders_locks_hash_budget, AZ-3908_token_store_device_login, AZ-3915_tray_autostart, AZ-3916_updater_verification, AZ-3918_hostile_names_escape, AZ-3959_http_sync_client_connect, AZ-3960_sync_tree_operations
**Component**: 14 desktop_daemon
**Tracker**: AZ-3917
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c19

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `HttpSyncClient::connect`, `ConnectError`, `list_children`, `create_folder` (E-23/c07, E-23/c08).
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — the trait, types and errors implemented here (E-24/c05).

## Problem

The settings window is stateless and the tray is thin: every visible behaviour of the daemon goes through `DaemonControl`. A changed setting must be confirmed in the response and take effect within the AC-16 budget; typed errors must be exact so 15 can show them next to the right field; the status snapshot must include holds, skipped and escaped counts, update state, `watcher_degraded` and (macOS later) `access_denied`. Nothing in this surface may return a token.

The planned row was 3 points for status, subscribe and settings echo. No other row owns the server and pair methods (`set_server`, `list_server_folders`, `add_pair` validations, `update_pair`, `remove_pair`, `pause`, `resume`, listings), so this task carries the whole implementation and is re-estimated at 5 points.

## Outcome

- Every method of the `ipc_schema` contract implemented with the behaviour of `daemon_control.md`.
- `status` assembles `server_url`, `account`, `pairs[]`, `skipped_count`, `escaped_count`, `holds[]`, `update_available`, `access_denied` and `watcher_degraded` from the index and the running parts; `subscribe` emits one full `StatusEvent` per change.
- `set_server`: `ServerUrl` check and `GET /api/v1/server-info` probe through sync-core, stored in `meta`, refused while signed in.
- `add_pair`: exactly one target; root checks (`RootInvalid`, `RootNested`), `AlreadyPaired`, `UnknownFolder`, `CloudManagedRoot` (E-24/c14), `RulesInvalid{line}` (E-24/c09); `create_if_missing` creates the server path; the case-insensitivity probe result stored; root sentinel written (E-24/c11); first-sync merge started for non-empty sides (E-24/c13).
- `update_pair` (rules), `remove_pair`, `pause`, `resume`; `list_skipped`, `list_escaped` paged from the index; `resolve_hold` handed to the guard (E-24/c11); `login_start`, `login_status`, `logout` handed to E-24/c15.
- `get_settings` and `set_settings`: bandwidth limit, hash throttle, autostart (installer of E-24/c17) and tray on or off; a valid patch is applied at once, persisted in `meta` and echoed; an invalid key gives `SettingInvalid{key}` and changes nothing.
- The daemon's `ProgressSink` for sync-core, feeding the pair state shown in status.

## Scope

### Included
- The implementation of all 18 methods; status assembly and event emission; settings persistence and live apply.

### Excluded
- The endpoint and dispatch (E-24/c16); the logic it delegates to (guard, login, merge, rules, placeholders, updater); the settings window (E-25); spawning parts in the running daemon (E-24/p01).

## Acceptance Criteria

**AC-1: Server address errors (AC-54)**
Given `set_server` with `http://x`, an unreachable host, a server whose `min_client_version` is above this build, and a call while signed in
When each is called
Then the results are `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}` and `SignedIn`, and a valid https address is stored and shown in `status.server_url`

**AC-2: Server-folder listing**
Given `list_server_folders` with no parent, a valid folder, an unknown node, and while signed out
When each is called
Then the first two return folders only (paged), the third `UnknownFolder`, the fourth `NotSignedIn`

**AC-3: Pair validation**
Given `add_pair` with a nested root, an unwritable root, the same server folder twice, an unknown node, a root inside a cloud-managed folder, rules with a bad line, and a `create_if_missing` path
When each is called
Then the results are `RootNested`, `RootInvalid`, `AlreadyPaired`, `UnknownFolder`, `CloudManagedRoot`, `RulesInvalid{line}`, and for the last the server folder is created and a `PairId` returned with the probe result stored

**AC-4: Unknown pair and no hold**
Given an unknown `PairId`, and a pair with no hold
When `update_pair`, `remove_pair`, `pause`, `resume`, `list_skipped`, `list_escaped` and `resolve_hold` are called
Then each returns `UnknownPair`, and `resolve_hold` on the pair without a hold returns `NoSuchHold`

**AC-5: Settings echo and effect (AC-16)**
Given a running transfer
When `set_settings` sets a bandwidth cap, and separately bandwidth -1
Then the first returns the applied value and the cap is visible on the transfer within 1 s; the second returns `SettingInvalid{key}` and settings are unchanged

**AC-6: Status and events**
Given a subscribed client
When a pair is paused and resumed, and a hold is entered
Then a `StatusEvent` with the full status arrives for each change, and `status` contains every field of the contract with no token in it

**AC-7: Logout and login delegation**
Given `login_start` with no server set, and `logout` with the server down
When they are called
Then `login_start` returns `ServerNotSet`; `logout` deletes the local token and reports that the server copy still needs revoking; no response contains a token

**AC-8: IPC latency (PT-14-4)**
Given 100 sequential `set_settings` calls while a transfer runs
When latencies are measured
Then the response p95 is at most 200 ms (failure above 1 s) and each value is effective within 1 s (failure above 5 s)

## Non-Functional Requirements

**Performance**
- Every method answers within 5 s (15's request timeout); `status` never waits on the network.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Validation matrix with fake `PlatformFs` and fake `SyncApi` | One typed error per single fault |
| AC-4 | Unknown pair across all pair methods; no hold | `UnknownPair`; `NoSuchHold` |
| AC-5 | Patch with a bad key and good keys | Nothing applied; `SettingInvalid{key}` |
| AC-6 | Event emission per state change | One full `StatusEvent` each |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-7 | Server for success paths; IT-14-15 inputs | IT-14-15 call matrix | Documented output or typed error per call | AC-16, AC-54, FT-P-49 |
| AC-8 | 100 `set_settings` during a transfer | PT-14-4 | p95 <= 200 ms; effective <= 1 s | AC-16, NFT-PERF-26 |

## Constraints

- IT tier: the implementation runs in-process behind the real IPC server with the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- Settings live in `meta`; the settings window keeps no state.

## Risks & Mitigation

**Risk 1: Slow `status` under load**
- *Risk*: counting skipped and escaped entries scans the index on every event.
- *Mitigation*: counts come from the index's partial state index; PT-14-4 runs during a transfer.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Row re-estimated from 3 to 5 points: the server and pair methods had no other owner | E-24 | accepted-risk | Low |
| Who enforces the bandwidth limit during transfers is not stated (13 has no bandwidth setting); unit open (G-AS-2) | E-24 / E-23 / owner | open | Medium |
| Per-file errors (FT-N-51, mockup "Errors") have no field in `DaemonStatus` (`ipc_schema.md` open point 2) | E-24 with owner | open | Medium |
| Which `DaemonStatus` field shows transfer progress fed by `ProgressSink` is not named | E-24 | open | Low |
| `remove_pair` keeping all files is derived, not stated (`daemon_control.md` open point 1) | E-24 | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
