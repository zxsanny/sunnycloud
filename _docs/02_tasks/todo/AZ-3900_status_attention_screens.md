# Status and Attention screens, status refresh, and the daemon-not-running view

**Task**: AZ-3900_status_attention_screens
**Name**: Status, Attention and daemon-down
**Description**: Keep the window's model as the last `DaemonStatus` (one `status` at open, `subscribe`, a 2 s fallback poll), show the Status tab (sync state, update available, rescan-only, skipped and renamed lists), the Attention tab (hold counts and the two decisions, root missing) and the "isn't running" view with its start button
**Complexity**: 5 points
**Dependencies**: AZ-3888_stub_daemon_ui_driver, AZ-3890_daemon_client, AZ-3895_settings_app_shell, AZ-3909_native_dialogs, AZ-3915_tray_autostart
**Component**: 15 desktop_settings
**Tracker**: AZ-3900
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `status`, `subscribe`, `StatusEvent`, `DaemonStatus`, `HoldView`, `list_skipped`, `list_escaped`, `resolve_hold`, `HoldDecision`, `UpdateView` (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — event emission and `resolve_hold` effects (E-24/c19).

## Problem

The window has no state of its own; what it shows is the last `DaemonStatus` it received (15 §1). It must follow the daemon closely (a change within 1 s of an event) without polling while events flow, and must fall back to polling when they stop. The Attention tab is where the user answers a mass-delete hold (AC-29): two decisions, the destructive one only after a confirmation. The Status tab must show that an update exists but offer no way to change the update source (AC-49). When the daemon is absent, the window must say so and start it the way login does, never by spawning a daemon itself. The epic row plans 3 points; the estimate is raised to 5 because the task holds four views plus the refresh policy.

## Outcome

- The model refreshes by events with one `status` at open and a 2 s +/- 0.5 s poll only after events stop.
- Status shows the sync state, update available, rescan-only notes, and paged skipped and renamed lists.
- Attention maps the two buttons to `resolve_hold`; a declined confirmation sends nothing.
- The daemon-not-running view runs the autostart start command and connects when the daemon is ready.

## Scope

### Included
- Refresh policy: one `status` call at open, `subscribe`, no polling while events flow, `status` every 2 s once events stop, back to events when they resume.
- Status tab (mockup Status tab): sync state and counts; "Update available" with the version from `UpdateView`; a rescan-only note for each pair in `watcher_degraded`; "Skipped" from `list_skipped` (path and reason), "Renamed on this computer" from `list_escaped` (original name next to the saved name), both paged.
- Attention tab (mockup Attention tab): per hold the pair, the local and remote delete counts, "Restore from server" (`resolve_hold(pair, RestoreFromServer)`) and "Delete on server too" (`resolve_hold(pair, ConfirmDeletes)` only after the native confirm returns true); `NoSuchHold` shows a message and the hold leaves after the next status; root missing shows the explanation and no delete button.
- Daemon-not-running view (mockup state "Daemon not running"): message, "Start sunnycloud" running the autostart start command, the note that the window connects as soon as the daemon is ready; the window never spawns a daemon process.
- Accessible names and roles for every control.

### Excluded
- The connection and reconnect logic (E-25/c05) and the confirm dialog itself (E-25/c12).
- The macOS access-denied message [v1.1] (E-25/c15).
- The per-file "Errors" list (flagged: no carrier in `ipc_schema`).

## Acceptance Criteria

**AC-1: Event-driven refresh with fallback (IT-15-9)**
Given a stub that counts requests, emits `StatusEvent`s for 10 s and then stops for 10 s
When the window is open the whole time
Then exactly 1 `status` call happens at open, at most 1 more while events flow, `status` is called every 2 s +/- 0.5 s after events stop, and the UI changes within 1 s of each event

**AC-2: Status content (IT-15-2, IT-15-13)**
Given a stub status with 2 pairs (active and listed in `watcher_degraded`, paused), `skipped_count` 3, `escaped_count` 2 and `update_available` with a version
When the Status tab opens
Then it shows the sync state, the update version, the rescan-only note for the first pair, 3 skipped entries with their reasons and 2 renamed entries each with the original name next to the saved name

**AC-3: Update shown, source not settable (AC-49, IT-15-13)**
Given `update_available` set
When every screen is walked through the accessibility tree
Then "update available" is shown, no control, field or menu accepts an update URL or key, and the window makes no download request

**AC-4: Hold decisions (AC-29, IT-15-6)**
Given a stub status with a mass-delete hold on pair P1 (200 local deletes) and a pair in root missing
When the user presses "Restore from server", then "Delete on server too" and accepts the confirmation
Then the counts are shown, the first sends `resolve_hold(P1, RestoreFromServer)`, the second sends `resolve_hold(P1, ConfirmDeletes)` only after the confirmation returned true, and the root-missing entry shows its explanation with no delete button

**AC-5: Declined confirmation and no hold (IT-15-6)**
Given the same hold
When the user presses "Delete on server too" and declines the confirmation, and later the stub answers a decision with `NoSuchHold`
Then the declined case sends no call, and `NoSuchHold` shows a message and the hold disappears after the next status

**AC-6: Daemon not running (IT-15-10)**
Given no daemon endpoint and a process spy
When the window opens and the user presses "Start sunnycloud", and the stub endpoint then appears
Then the view says the daemon is not running, the spy records the autostart start command line, the window itself starts no daemon process, and the window loads the status once the endpoint exists without a restart

## Non-Functional Requirements

**Performance**
- UI change within 1 s per event; fallback poll 2 s +/- 0.5 s (epic NFR, IT-15-9).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Refresh policy with a fake clock: events flowing, events stopped, events resumed | No poll while events flow; 2 s polls after they stop |
| AC-4 | Decision mapping and confirm gate | `ConfirmDeletes` only after confirm true |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stub event script | 20 s window open | 1 `status` at open; 2 s polls after stop; change <= 1 s | IT-15-9 |
| AC-2 | Stub status with lists | Open Status | Content as listed | IT-15-2, IT-15-13 |
| AC-3 | `update_available` | Walk all screens | No update-source control; no download | IT-15-13, AC-49 |
| AC-4 | Hold on P1; root missing | Both buttons; confirm | Two calls mapped; no delete for root missing | IT-15-6, FT-P-59 (UI part) |
| AC-5 | Hold; `NoSuchHold` | Decline; decide | No call; message; hold leaves | IT-15-6 |
| AC-6 | No endpoint; process spy | Start button; stub appears | Command recorded; no daemon spawned; status loads | IT-15-10 |

## Constraints

- The window never starts a second daemon itself (15 §7).
- Daemon text (paths, names, reasons) is rendered as plain text (15 §11).

## Risks & Mitigation

**Risk 1: Deletes confirmed by accident**
- *Risk*: a single click sends deletes to the server.
- *Mitigation*: AC-4 and AC-5 require the native confirmation first; E-25/c14 runs AT-15-2 against the real daemon.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The Status "Errors" list of 15 §5 and the mockup has no carrier in `DaemonStatus` or any method (`ipc_schema.md` open point 2); it is not built until E-24 adds one | E-24 / owner | open | Medium |
| The autostart start command per OS (systemd user unit, logon task) is defined by E-24/c17, but how the window learns it while the daemon is down is not stated | E-24 / E-25 owners | open | Medium |
| `RestoreFromServer` for a hold caused by 200 or more remote deletes is undefined (`daemon_control.md` open point 2) | E-24 / owner | open | Low |
| Estimate raised from 3 to 5 points (four views plus the refresh policy) | E-25 | open | Low |
| `components/15_desktop_settings/tests.md` IT-15-9 still notes a missing `subscribe` in the client table; 15 §2 now lists it (doc fix) | Doc owner | open | Low |
