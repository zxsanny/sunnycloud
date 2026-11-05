# Settings app shell: software-renderer build, threading, single instance, window frame

**Task**: AZ-3895_settings_app_shell
**Name**: Settings app shell
**Description**: The `sunnycloud-settings` executable: a separate process with the Slint software renderer and the release profile of ADR-012, the UI on the main thread and the daemon client on its own thread, a single-instance lock, the six-tab window frame, the shared error display and the About dialog
**Complexity**: 3 points
**Dependencies**: AZ-3882_settings_cold_start_spike, AZ-3886_screen_reader_spike, AZ-3890_daemon_client, AZ-3914_ipc_server_endpoint, AZ-3917_daemon_control_impl, AZ-3652_initial_structure
**Component**: 15 desktop_settings
**Tracker**: AZ-3895
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `Internal{correlation_id}` and error kinds shown by the shared error display (E-24/c05).

## Problem

The window must open fast on weak hardware (AC-16), stay out of the daemon's memory budget (AC-15) and never block its UI while the daemon is slow. A second launch from the tray or the start menu must not open a second window. Every screen needs the same frame (the six tabs of the approved mockup) and the same way of showing errors: typed messages beside the field, unknown errors as a generic message plus the daemon's correlation id, all as plain text.

## Outcome

- A separate executable that opens the six-tab window and connects through the daemon client without blocking the UI.
- A second launch raises the first window; closing the window leaves the daemon untouched.
- One shared error display used by every screen.

## Scope

### Included
- The executable built with the software renderer only and the release profile of ADR-012.
- UI on the main thread; the daemon client (E-25/c05) on its own thread; results handed to the UI thread.
- Single-instance lock in the user runtime dir; a second launch brings the first window forward and exits.
- The window frame of `ui_mockups/desktop-settings.html`: tabs Account, Folders, Rules, Settings, Status, Attention (with a count of items waiting), light and dark theme tokens following the OS (DESIGN.md §7), flat surfaces only (no transforms, no layer opacity, no blurred shadows).
- The shared error display: a typed message beside the causing control, a generic message plus the correlation id for unknown errors, all daemon text rendered as plain text.
- The About dialog naming Slint, with the GPL-3.0 notices.
- INFO log lines for open and close; nothing persisted (no window geometry, no last tab).

### Excluded
- The screens' content (E-25/c07..c11) and the daemon-not-running view (E-25/c11).
- The tray menu: drawn by the OS and filled by the daemon (E-24/c17).
- The dependency gate in CI (E-25/c13).

## Acceptance Criteria

**AC-1: Separate process (AC-15, IT-15-11)**
Given the real daemon running and syncing a pair
When the window is opened, used for a minute and closed
Then the window has its own PID, the daemon's RSS stays within +/- 2 MB of its value before opening, the IPC connection closes on exit and the daemon keeps syncing

**AC-2: Single instance (IT-15-11)**
Given an open window
When the executable is launched a second time
Then one settings process remains, the first window is brought forward, and the lock lives in the user runtime dir

**AC-3: UI stays responsive (IT-15-1)**
Given a stub daemon that delays an answer by 4 s
When the user switches tabs and types into a field while the call is pending
Then the tab switch and the typing take effect before the answer arrives

**AC-4: Shared error display (IT-15-1, ST-15-1)**
Given an unknown error with correlation id `c-77`, a typed error and error text `<b>x</b>{@html}<script>`
When each is shown
Then the unknown error shows a generic message and `c-77`, the typed error shows its message with no raw code string, and the markup text appears literally

**AC-5: Nothing persisted**
Given a window moved, resized and switched to another tab
When it is closed and opened again
Then it opens in the same initial state as a first start, and no file other than logs is written in the user's config or state directories

**AC-6: About dialog**
Given the About dialog
When it is opened
Then it names Slint and shows the GPL-3.0 notices

**AC-7: Starts without workarounds (AC-16)**
Given a release build on H-LIN and H-WIN with no `SLINT_BACKEND` or other renderer variable set
When the window starts
Then it renders with the software renderer

## Non-Functional Requirements

**Performance**
- Cold-start targets of PT-15-1 (p95 <= 300 ms target, fail > 1 s); checked on the finished binary by E-25/c14.

**Compatibility**
- Linux (X11 and Wayland) and Windows 11; macOS in v1.1.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Lock acquire, held, released after exit | Second acquire fails while held; succeeds after exit |
| AC-4 | Error-to-message mapping for known, unknown and markup text | Typed message; generic + id; literal text |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Real daemon, one pair syncing | Open, use, close | Distinct PID; daemon RSS +/- 2 MB; daemon keeps syncing | IT-15-11, NFT-RES-LIM-06 |
| AC-2 | Window open | Second launch | One process; first window raised | IT-15-11 |
| AC-3 | Stub delays 4 s | Tab switch and typing | Handled before the answer | IT-15-1 |
| AC-4 | Stub error scripts | Show errors | Generic + `c-77`; no raw code; literal markup | IT-15-1, ST-15-1 |
| AC-5 | Moved window, other tab | Close and reopen | First-start state; no state file | 15 §4 |
| AC-6 | Window | Open About | Slint and GPL notices | DoD |
| AC-7 | Release build, no env vars | Start | Software renderer | AC-16 |

## Constraints

- Never linked into the daemon; imports only `ipc_schema` and helper config (module-layout, ADR-012).
- The UI owns the main thread; the client's runtime is a single-threaded one on its own thread (15 §5).

## Risks & Mitigation

**Risk 1: Gate S3 fails**
- *Risk*: E-25/c01 fires fallback trigger 1.
- *Mitigation*: this task starts after E-25/c01; the IPC client is unchanged under a Tauri shell (ADR-012).

## Runtime Completeness

- Named capability: Slint with the software renderer.
- Production code that must exist: the real window rendered by the software renderer, the real single-instance lock.
- Allowed external stubs: the stub daemon for AC-3 and AC-4.
- Unacceptable substitutes: a GL renderer build, a headless build for the automated checks.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The Attention tab count ("2 items" in the mockup) is not defined by 15 §5; this task counts the entries of `DaemonStatus.holds` (mass delete and root missing) | E-25 / doc owner | open | Low |
