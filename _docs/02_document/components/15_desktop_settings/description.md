# 15_desktop_settings

## 1. High-Level Overview

**Purpose**: A small native window, started on demand, in which the user manages the account, sync pairs, rules and a few settings of the running daemon.

**Single responsibility**: presentation and input for daemon settings. It holds no state of its own, performs no sync, no hashing, no network call to the server and never sees a token. Every action is one JSON-RPC call to 14_desktop_daemon.

**Architectural pattern**: stateless view over a request/response client (MVVM-lite: `.slint` views bound to a `SettingsViewModel`; the model is the last `DaemonStatus` received).

**Upstream dependencies**: 14_desktop_daemon (JSON-RPC over the local socket or pipe; the `ipc_schema` crate, which 14 owns), helper 01 config (only the IPC endpoint path rule).
**Downstream consumers**: the user; spawned by the tray menu or by the installer's start-menu entry.

### Scope
In: Slint UI with the software renderer only, `rfd` native folder/file dialogs, JSON-RPC client, pair and rule editing, account sign-in/out display, status, holds, skipped/escaped lists, settings list, single-instance guard, accessibility properties.
Out: any file browser (AC-17), token handling, server calls, autostart logic (the daemon does it), updater logic (the daemon verifies the offline-signed manifest from its build-time source against two pinned keys; this window only shows "update available" and has no setting for the update source), vault.

## 2. Internal Interfaces

### Interface: DaemonClient (JSON-RPC 2.0, newline-delimited; runs on a separate thread with its own tokio current-thread runtime)

| Method (mirrors DaemonControl) | Input | Output | Async | Error Types |
|--------|-------|--------|-------|-------------|
| `status` | none | `DaemonStatus` (includes `access_denied`, `watcher_degraded`) | Yes | `DaemonNotRunning`, `EndpointUntrusted` |
| `set_server` | server address typed in the Account screen | `()` | Yes | `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}`, `SignedIn` |
| `list_server_folders` | `parent` (none = root) | `[FolderEntry{node_id, name}]` for the server-folder picker | Yes | `NotSignedIn`, `ServerUnreachable`, `UnknownFolder` |
| `list_pairs` | none | pairs with state for the Folders screen | Yes | `NotSignedIn` |
| `subscribe` | none | stream of `StatusEvent`, each carrying the new `DaemonStatus` (including `access_denied` and `watcher_degraded`, shown in the status area) | Yes | `DaemonNotRunning` |
| `add_pair` / `update_pair` / `remove_pair` / `pause` / `resume` | specs from the form (server folder by picked node id, or by typed path with create-if-missing) | `PairId` or `()` | Yes | `NotSignedIn`, `RootInvalid`, `RootNested`, `RulesInvalid{line}`, `CloudManagedRoot`, `AlreadyPaired`, `UnknownFolder` |
| `list_skipped` / `list_escaped` | optional `PairId`, page | skipped entries (path, reason) / escaped names (original, saved) | Yes | `UnknownPair` |
| `get_settings` / `set_settings` | `SettingsPatch` | applied `Settings` | Yes | `SettingInvalid{key}` |
| `resolve_hold` | `PairId`, decision | `()` | Yes | `NoSuchHold` |
| `login_start` / `login_status` / `logout` | device name | `LoginPrompt` (URL and user code, never a token) | Yes | `ServerNotSet`, `ServerUnreachable`, `HttpsRequired` |

Typed errors are shown as localized messages next to the field that caused them; unknown errors show a generic message plus the daemon's correlation id.

### Interface: NativeDialogs
`pick_folder() -> Option<PathBuf>`, `pick_file() -> Option<PathBuf>`, `confirm(text) -> bool` via `rfd` `AsyncFileDialog` awaited inside `slint::spawn_local` (Slint's loop drives the future). No custom path browser anywhere; a text field shows the chosen path read-only.

## 2a. Ports

Same format as 13 (client-side traits live in `sync-core`; across a process boundary the named schema applies; `DaemonControl` is in `ipc_schema`).

| Direction | Port | Defined in | Counterpart |
|---|---|---|---|
| Requires | `DaemonControl` (JSON-RPC 2.0, section 2) | `ipc_schema` (14's crate; process boundary) | 14_desktop_daemon |
| Provides | none | | |

## 3. External API
None.

## 4. Data Access
Owns no data. Window geometry and last-selected tab are not persisted (nothing to migrate, nothing to leak). Daemon status is refreshed by `subscribe` events plus one `status` call at open and a timer refresh at `settings.refresh_interval` (2s) as a fallback when events stop.

## 5. Implementation Details

**Process model**: separate executable (`sunnycloud-settings`), spawned by the daemon on "Open settings" or by the shortcut, never linked into the daemon (AC-15 excludes it from the daemon's memory budget). Single-instance guard: a second launch brings the first window forward through a lock in the user runtime dir. Exit closes the socket; the daemon is unaffected.

**Renderer**: Slint 1.18.1 with `default-features = false`, features `std`, `compat-1-18`, `backend-winit` (+ wayland, x11 on Linux), `renderer-software`, `accessibility`. No femtovg, no Skia: the measured GL path took 160-420 ms and 121-136 MB and did not fall back on an EGL failure, while the software build opened in 47-61 ms at about 17 MB with no environment variables (AC-16). Release profile: fat LTO, `opt-level = "s"`, `strip`, `panic = "abort"`. Licence: GPL-3.0-only route; third-party notices carry the GPL text and the About dialog names Slint.

**Threading**: Slint owns the main thread. IPC runs on a `std::thread` hosting a tokio current-thread runtime and posts results with `upgrade_in_event_loop`. `#[tokio::main]` is not used.

**Screens** (about six): 1 Account (server address field applied with `set_server`, then the sign-in button showing the verification URL and user code, sign-out). 2 Folders (list of pairs with state, add via the local folder dialog plus a server-folder picker fed by `list_server_folders`, or a typed server path that is created if missing; remove, pause). 3 Rules (per pair, one multi-line pattern field with live validation errors from `RulesInvalid{line}`; help text lists the grammar: `*`, `**`, `?`, leading `/`, trailing `/`, `!`). 4 Settings (below). 5 Status (sync state, skipped entries from `list_skipped`, `%XX`-escaped names with the original name next to each from `list_escaped`, errors, update available). 6 Attention (mass-delete hold with counts and two buttons "Restore from server" / "Delete on server too", root-missing explanation; macOS access-denied message in v1.1).

**Settings list**: bandwidth limit (number, 0 = unlimited); initial-hash throttle (on/off, uses the daemon default share); start at login (checkbox, daemon installs/removes the unit/task); show tray icon (checkbox; takes effect on next start where the tray host is absent); pause all; language (v1.0 English only). Changes apply by one `set_settings` call; the response echoes the applied value, which the UI shows, so a change is visible as applied within the 5 s budget (AC-16). No hidden state: if the call fails, the control reverts.

**Fallback to Tauri 2 + Svelte** (decided, not built): switch only when one of these is measured or reproduced: (1) cold start over 1 s at p95 on the weakest Windows laptop after binary trimming, or over 1 s on Ubuntu LTS Wayland (GNOME); (2) a reproducible Windows/macOS blocker with no upstream fix in a Slint patch release within the release window (text input/IME, window not shown, dialog not parented); (3) a screen-reader failure (NVDA/Narrator/VoiceOver cannot reach Change/Remove/checkbox/number field) not fixable with `accessible-*` properties; (4) rfd dialog deadlock or unusable focus under the winit loop; (5) a licence change that closes the GPL route. The IPC contract is identical, so a Tauri shell reuses `DaemonClient` logic unchanged.

**Key dependencies**: slint 1.18.1 (+ `slint-build`), rfd 0.17.2, interprocess 2.4.7, tokio (current-thread), serde. rfd on Linux needs an XDG portal backend or zenity (not wlroots portals): the package docs and an in-window message state this when `pick_folder` reports no backend.

## 6. Helpers
`ipc_schema` crate (request/response types, owned by 14, consumed here): single source for method names and errors, so a test can walk it for forbidden result types.

## 7. Caveats and Edge Cases
- Dialog runtime behaviour per OS is unverified (GNOME without zenity, wlroots without portal): spike per OS before release.
- Software renderer has no transforms/layer opacity; the UI avoids them.
- Fractional scaling (125/150/175%) and Windows per-monitor DPI are unmeasured.
- Daemon not running: the window shows "daemon not running" with a start button that runs the autostart command; it never starts a second daemon itself.
- Pipe/socket squatting: the client checks that the endpoint is owned by the current user before sending anything (Windows: pipe server SID; Unix: socket owner and mode) and refuses an untrusted endpoint (NFT-SEC-32 step 3).

## 8. Dependency Graph
Must be implemented after: 14's `ipc_schema` and `DaemonControl` (can start against a stub daemon). Parallel with: 14, 16, 17. Blocks: nothing.

## 9. Logging
ERROR: IPC protocol violations, dialog backend failures. WARN: endpoint untrusted, connection lost. INFO: opened/closed. No user paths at INFO, no tokens ever (none are received). Logs to the user state dir, same rotation as the daemon.

## 10. Configuration
No config file. Compile-time constants: refresh interval fallback 2s, request timeout 5s (matches the AC-16 apply budget), IPC reconnect backoff 250ms to 2s.

## 11. Security notes
The window never sees or asks for a token; sign-in happens in the system browser. The IPC client trusts only an endpoint owned by the current user. Pattern text and paths are sent as data; the daemon validates them. Error text from the daemon is shown as plain text (no markup). No network access at all: the binary has no HTTP client dependency (checked in CI by `cargo tree`).

## 12. Acceptance criteria implemented
AC-16 (opens within 1 s, setting applied within 5 s), AC-17 (native dialogs only), contributes AC-66 (pattern entry and error display), AC-29 (hold decision UI), AC-67 [v1.1] (access-denied message within 10 s of the daemon reporting it), AC-48 (client side of the endpoint trust check). All [v1.0] except AC-67.
**Interaction ACs folded in**: none are owned by this component.

## 13. Test scenarios
FT-P-48, FT-P-49, FT-P-50, FT-P-60 (UI part), FT-P-59 (prompt UI), FT-P-75 [v1.1], NFT-PERF-25, NFT-PERF-26, NFT-SEC-32 (steps 3 and 4 client side).

## 14. Component acceptance
- Cold start at most 1 s at p95 on a 2019 Windows laptop (HDD-class) and Ubuntu LTS Wayland + NVIDIA, no environment variables.
- Every file/folder choice opens the OS dialog (automated check that no widget lists directory contents).
- A changed setting is reflected as applied by the daemon within 5 s without restart.
- `cargo tree` for the binary shows no HTTP client and no femtovg/skia.
- Settings window is not counted in the daemon's 100 MB.

## 15. Risks
0. Default chosen at Step 5 gap pass: `DaemonStatus` has `access_denied` (AC-67, v1.1) and `watcher_degraded` (rescan-only fallback), shown in settings; `DaemonClient` table includes `subscribe` and `list_pairs`.
1. Windows cold start and fractional-DPI behaviour not measured (gate S3); fallback triggers above.
2. Screen-reader support of Slint widgets unverified.
3. rfd/portal availability on minimal Linux desktops.
4. Slint GPL-3.0-only route constrains any future proprietary build.
