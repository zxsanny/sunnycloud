# Test Specification — 15 Desktop Settings

Component tests complement the system-level scenarios (`02_document/tests/`) and reference them by ID. Tiers: **IT with stub daemon** = the real `sunnycloud-settings` binary driven by UI automation (AT-SPI on Linux, UI Automation on Windows) against a stub JSON-RPC server that implements `DaemonControl` from `ipc_schema` with scripted answers (the settings process has no server dependency, so a stub is the right boundary; env `C` on Linux containers with Xvfb/Wayland, `H-LIN`, `H-WIN`); **IT with real daemon** = the same binary against the real daemon (14) and the real server container with real PostgreSQL. Cold-start and DPI claims are valid only on REF-WIN-WEAK and REF-LIN-WAYLAND-NV, otherwise `NOT-RUN(HW-REF)`. Only `DaemonClient` methods and `NativeDialogs` from the description are used.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|-----------------------------|----------|----------|
| AC-15 | Settings window not counted in the daemon's memory (separate process) | IT-15-11 | Covered; system NFT-RES-LIM-06 |
| AC-16 | Window opens within 1 s; setting applied within 5 s | PT-15-1, PT-15-3, IT-15-5 | Covered; system FT-P-48, FT-P-49, NFT-PERF-25, NFT-PERF-26 |
| AC-17 | Native dialogs only; no custom file browser | IT-15-3 | Covered; system FT-P-50 |
| AC-29 | Hold decision UI | IT-15-6, AT-15-2 | Covered (UI part); system FT-P-59 |
| AC-48 | Client side of the endpoint trust check | IT-15-12, ST-15-1 | Covered; system NFT-SEC-32 (steps 3 and 4) |
| AC-54 | HTTPS-required error shown at the address field | IT-15-1 | Covered; system NFT-SEC-12 |
| AC-66 | Pattern entry and `RulesInvalid{line}` display | IT-15-4, AT-15-1 | Covered; system FT-P-60 (UI part) |
| AC-49 | Window shows "update available" and has no update-source setting | IT-15-13 | Covered (display half); verification is 14 |
| AC-67 | Access-denied message within 10 s of the daemon reporting it [v1.1] | IT-15-14 | BLOCKED — no `DaemonStatus` field carries the denial (see IT-14-24 in 14); system FT-P-75 |

## Blackbox Tests

### IT-15-1: Typed errors appear next to the field that caused them
- **Summary**: Each `DaemonControl` error is shown as a message beside the right control.
- **Traces to**: AC-54
- **Input**: stub daemon answers in turn: `set_server` -> `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message: "Please update to 1.2"}`, `SignedIn`; `add_pair` -> `RootInvalid`, `RootNested`, `CloudManagedRoot`, `AlreadyPaired`, `UnknownFolder`, `NotSignedIn`; `login_start` -> `ServerNotSet`; an unknown error code with correlation id `c-77`.
- **Expected**: `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired` (text "Please update to 1.2") and `SignedIn` messages sit beside the server address field; the `add_pair` errors beside the local-folder or server-folder control; `ServerNotSet` beside the sign-in button; the unknown error shows a generic message plus `c-77`; the UI stays responsive and no raw error code string is shown for known errors.
- **Max time / Deps**: 30 s; stub daemon.

### IT-15-2: The six screens and their controls
- **Summary**: Account, Folders, Rules, Settings, Status and Attention screens exist with the specified content and accessible names.
- **Traces to**: AC-16
- **Input**: stub status with 2 pairs (active, paused), `skipped_count` 3, `escaped_count` 2, one hold, `update_available`; AT-SPI/UIA tree dump.
- **Expected**: Account (address field, sign-in button, verification URL and user code area, sign-out); Folders (pair list with state, add, remove, pause); Rules (multi-line pattern field); Settings (bandwidth number, hash throttle on/off, start at login, show tray icon, pause all; language English only); Status (sync state, skipped list, escaped names with the original next to each, errors, update); Attention (counts and two buttons). Each of Change, Remove, checkbox and number field has an accessible name and role.
- **Max time / Deps**: 60 s; stub daemon, AT-SPI (H-LIN) / UIA (H-WIN); NVDA/Narrator reachability is a manual check (risk 2 in the description).

### IT-15-3: Only native dialogs choose paths
- **Summary**: Folder and file choices open the OS dialog and no custom browser exists.
- **Traces to**: AC-17
- **Input**: click "Add folder" and any file chooser; Linux: observe the `org.freedesktop.portal.FileChooser` D-Bus call; Windows: find a `#32770` common dialog; walk the whole accessibility tree for list or tree widgets that contain directory entries; Linux without portal backend and zenity.
- **Expected**: dialog opens for every path choice (`pick_folder`, `pick_file`); the chosen path appears in a read-only text field; 0 widgets in the tree list directory contents; with no backend the window shows a message that names the XDG portal or zenity requirement and does not crash.
- **Max time / Deps**: 60 s; LIN-VM, WIN-VM; system FT-P-50.

### IT-15-4: Rules editor with live validation
- **Summary**: Invalid pattern lines are marked from `RulesInvalid{line}` and the grammar help is shown.
- **Traces to**: AC-66
- **Input**: type `*.log`, `build/`, `!keep.log`, `/x`, `a/**/b?`; the stub answers `RulesInvalid{line: 3}` for a text whose third line is `[`.
- **Expected**: error marker on line 3 only, other lines unmarked; Save disabled while an error exists; help lists `*`, `**`, `?`, leading `/`, trailing `/`, `!`; text is sent to the daemon unchanged (stub records the exact string, including trailing newline and non-ASCII).
- **Max time / Deps**: 30 s; stub daemon.

### IT-15-5: Settings apply, echo and revert
- **Summary**: A change is one `set_settings` call, the shown value is the daemon's echo, and a failure reverts the control.
- **Traces to**: AC-16
- **Input**: change bandwidth to 5000, toggle autostart; stub echoes `Settings` with bandwidth 4096 (clamped); then answers `SettingInvalid{key: bandwidth}`; then the stub does not answer within 5 s.
- **Expected**: exactly 1 `set_settings` per change; control shows 4096 (the echo); `SettingInvalid` -> control reverts to the last applied value and a message names the key; timeout at 5 s -> control reverts with a "daemon did not answer" message; no hidden state (reopening shows the daemon's values).
- **Max time / Deps**: 30 s; stub daemon with scripted delays.

### IT-15-6: Attention screen and hold decision
- **Summary**: A mass-delete hold shows counts and two decisions mapped to `resolve_hold`.
- **Traces to**: AC-29
- **Input**: stub status with a hold on pair P1 (200 deletes, direction local) and a `root_missing` pair; click each button.
- **Expected**: counts displayed; "Restore from server" -> `resolve_hold(P1, RestoreFromServer)`; "Delete on server too" -> `resolve_hold(P1, ConfirmDeletes)` only after a confirmation (`confirm(text)` returns true; a declined confirm sends nothing); `NoSuchHold` -> message and the hold disappears after the next status; root-missing text explains the cause and offers no delete button.
- **Max time / Deps**: 30 s; stub daemon; real-daemon variant AT-15-2.

### IT-15-7: Server-folder picker and typed path
- **Summary**: The pair form builds `PairSpec` with exactly one of node id and typed path.
- **Traces to**: AC-16
- **Input**: stub `list_server_folders` returning 3 pages of folders (paged), picking a nested folder; typing `Photos/2026` with "create if missing"; picking and also typing.
- **Expected**: `add_pair` carries `server_node_id` for the picked folder or `server_path` with `create_if_missing=true` for the typed one, never both; the form refuses to submit when both are set; paging loads further pages on scroll; `UnknownFolder` clears the selection with a message.
- **Max time / Deps**: 30 s; stub daemon.

### IT-15-8: Sign-in display
- **Summary**: Sign-in shows the verification URL and user code and never a token.
- **Traces to**: AC-54
- **Input**: stub `login_start` -> `LoginPrompt{verification_url, user_code: "WDJB-MJHT", expires_at}`; `login_status` Pending x3, then Approved; separate runs for Denied, Expired; `logout`.
- **Expected**: URL and code visible and copyable; status text follows `LoginState`; after Approved the Account screen shows the account and sign-out; Denied and Expired show a message and a retry; the window never asks for a password or token; `logout` shows the server-copy warning when the stub says so.
- **Max time / Deps**: 30 s; stub daemon.

### IT-15-9: Status refresh by events with timer fallback
- **Summary**: The model is the last `DaemonStatus`, refreshed by events, one call at open and a 2 s fallback.
- **Traces to**: AC-16
- **Input**: stub counts requests; emit `StatusEvent`s for 10 s; then stop events for 10 s.
- **Expected**: 1 `status` call at open; while events flow no polling `status` calls (at most 1 extra); after events stop `status` is called every 2 s (+/- 0.5 s); the UI changes within 1 s of each event. Spec gap: the `DaemonClient` table lists no `subscribe` method though section 4 relies on it; the test uses `subscribe` as in `DaemonControl`.
- **Max time / Deps**: 40 s; stub daemon.

### IT-15-10: Daemon not running; reconnect
- **Summary**: The window shows "daemon not running" with a start button and never starts a second daemon itself.
- **Traces to**: AC-16
- **Input**: no endpoint; start the window; then bring the stub up after 3 s; click the start button.
- **Expected**: message `DaemonNotRunning` shown; reconnect attempts back off from 250 ms to 2 s; the button runs the autostart command (spy records the command line) and the window itself spawns no daemon process; after the endpoint appears the window loads status without restart.
- **Max time / Deps**: 30 s; stub daemon, process spy.

### IT-15-11: Process model and single instance
- **Summary**: A second launch raises the first window; closing the window leaves the daemon unaffected; the window is a separate process.
- **Traces to**: AC-15
- **Input**: launch twice; close the window; check the real daemon's RSS and pairs before and after.
- **Expected**: 1 settings process remains, the first window is brought forward; the lock lives in the user runtime dir; after exit the socket is closed and the daemon keeps syncing; the settings process is not a child thread of the daemon (distinct PIDs), so the daemon's RSS is unchanged (+/- 2 MB) while the window is open.
- **Max time / Deps**: 30 s; real daemon.

### IT-15-12: Untrusted endpoint is refused
- **Summary**: The client verifies endpoint ownership before sending anything.
- **Traces to**: AC-48
- **Input**: a squatter listens at the endpoint path owned by another user (Linux: wrong owner, mode 0666; Windows: pipe whose server SID differs); a squatter with correct path but group-writable directory.
- **Expected**: `EndpointUntrusted` shown; the squatter receives 0 bytes; a log WARN names "endpoint untrusted"; the window offers no retry that bypasses the check.
- **Max time / Deps**: 30 s; second OS user; system NFT-SEC-32 steps 3 and 4.

### IT-15-13: Update available display; no update-source setting
- **Summary**: The window only shows that an update exists.
- **Traces to**: AC-49
- **Input**: stub status with `update_available`; walk all screens.
- **Expected**: Status screen shows "update available"; no control, field or menu allows entering an update URL or key; the window makes no download request.
- **Max time / Deps**: 20 s; stub daemon.

### IT-15-14: macOS access-denied message [v1.1]
- **Summary**: The window shows "access denied: allow access in System Settings" within 10 s of the daemon reporting it.
- **Traces to**: AC-67
- **Expected**: BLOCKED — the daemon exposes no status field or hold kind for a permission denial (14 `DaemonStatus`, IT-14-24), so there is nothing for the window to render. Needed: the daemon-side field first; then the test sends it from the stub and measures render time <= 10 s. System FT-P-75 stays H-MAC.
- **Max time / Deps**: n/a; MAC, v1.1.

## Performance Tests

### PT-15-1: Cold start (gate S3)
- **Summary**: The window opens within 1 s at p95 on the reference machines with no environment variables.
- **Traces to**: AC-16
- **Load**: 30 cold starts per machine (cache dropped / fresh boot), measured from process start to first rendered frame; REF-WIN-WEAK (2019 laptop, HDD-class disk, Defender on) and REF-LIN-WAYLAND-NV; software renderer build.
- **Targets / failure**: p95 <= 300 ms / above 1 s; max <= 600 ms / above 1.5 s. Measured in the spike: 47 to 61 ms. VM results are informational. System NFT-PERF-25.

### PT-15-2: Memory and size of the window process
- **Summary**: The software-renderer build stays small.
- **Traces to**: AC-15
- **Load**: window open on Status screen for 5 min with events at 1/s.
- **Targets / failure**: RSS <= 30 MB (spike: about 17 MB) / above 60 MB; no growth above 2 MB over 5 min. Targets are proposed by this spec; the AC only excludes the window from the daemon's 100 MB.

### PT-15-3: Setting reaches the daemon
- **Summary**: A changed setting is applied by the real daemon within the budget.
- **Traces to**: AC-16
- **Load**: 20 changes (bandwidth, hash throttle, tray, autostart) against the real daemon during a transfer.
- **Targets / failure**: UI shows applied <= 1 s / above 5 s; effect observable in the daemon (bandwidth cap on the wire) <= 1 s / above 5 s; 0 restarts. System NFT-PERF-26, FT-P-49.

## Security Tests

### ST-15-1: Endpoint squatting and hostile daemon answers
- **Summary**: The window trusts only an endpoint owned by the current user and treats daemon text as data.
- **Traces to**: AC-48
- **Attack vector**: squatted pipe/socket (IT-15-12); a malicious server on a trusted endpoint answering with oversize JSON (10 MB), invalid JSON, wrong ids, and error text `<b>x</b>{@html}<script>`.
- **Procedure**: run the stub with each answer.
- **Pass**: the window shows the text literally (no markup rendering), rejects malformed frames with a "protocol violation" log at ERROR, stays alive and releases memory. **Fail**: crash, hang over 5 s, rendered markup.

### ST-15-2: No network, no tokens
- **Summary**: The binary has no HTTP client and never receives a token.
- **Traces to**: AC-48
- **Attack vector**: dependency inspection and traffic capture.
- **Procedure**: `cargo tree` for the release binary (CI job); run the full AT set inside a network namespace with no interfaces except loopback IPC; capture the IPC traffic of a signed-in session.
- **Pass**: no `reqwest`, `hyper`, `ureq` or other HTTP client, no `femtovg`/`skia`; all tests pass offline; token value absent from captured traffic. **Fail**: any dependency or token hit.

### ST-15-3: Logs carry no secrets or user paths
- **Summary**: INFO logs contain no user file paths and no user codes.
- **Traces to**: AC-48
- **Attack vector**: log inspection after IT-15-3, IT-15-8.
- **Procedure**: run with sentinel folder `/home/u/SECRET-FOLDER` and user code `WDJB-MJHT`; grep the log at INFO.
- **Pass**: 0 matches. **Fail**: 1 or more.

## Acceptance Tests

### AT-15-1: Add a folder pair with rules
- **Summary**: A user pairs a folder end to end.
- **Traces to**: AC-17, AC-66
- **Preconditions**: real daemon signed in, empty server account.
- **Steps**: (1) open the Folders screen, click Add: the OS folder dialog appears; (2) pick a local folder and a server folder by the picker: the pair appears as active; (3) enter rules `*.tmp` and a bad line: error marked on that line; fix it and save: accepted; (4) create `x.tmp` and `y.txt` locally: only `y.txt` reaches the server.

### AT-15-2: Mass-delete hold from the window
- **Summary**: The Attention screen resolves a hold.
- **Traces to**: AC-29
- **Preconditions**: 300 synced files (DS-23), real daemon.
- **Steps**: (1) delete 200 files locally: Attention shows the hold and counts, server unchanged; (2) click "Restore from server": the 200 files return with equal hashes and the hold clears; (3) repeat the deletion and click "Delete on server too" and confirm: the 200 files are deleted on the server. System FT-P-59.

### AT-15-3: Change a setting
- **Summary**: A setting is applied without restart.
- **Traces to**: AC-16
- **Preconditions**: real daemon, a running transfer.
- **Steps**: (1) set bandwidth to 5,000 and press Apply: the control shows the echoed value within 1 s; (2) the transfer rate falls to the cap within 5 s; (3) close and reopen the window: the same value is shown. System FT-P-49.

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| Stub scripts | scripted `DaemonControl` answers (errors, pages, events, delays) | `tests/ui/stub-daemon/*.yaml` | < 1 MB |
| DS-23 | 300 files for the hold test | test-data.md | 10 MB |
| Pattern fixtures | valid and invalid rule texts, non-ASCII | repository fixture | < 100 KB |

**Setup**: stub tests need no server; start the stub on a temp endpoint, launch the binary with that endpoint. Real-daemon tests: `tests/run.sh --profile desktop`, account via invite token, daemon signed in through the device grant; lab controller reverts VM snapshots for cold-start runs.
**Teardown**: kill the window, stub and daemon; delete temp endpoints and state dirs; purge the account via `e2e-ctl`.
**Isolation**: one stub endpoint and runtime dir per test; the window keeps no state (no geometry or tab persisted), so tests do not influence each other; a fake daemon is allowed here because it sits at the process boundary the component owns, while tests that need real sync use the real daemon and server with real PostgreSQL.
