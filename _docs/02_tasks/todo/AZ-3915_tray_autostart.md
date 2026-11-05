# Tray and autostart installers (systemd user unit, Windows logon task)

**Task**: AZ-3915_tray_autostart
**Name**: Tray and autostart
**Description**: An optional tray icon and menu over `DaemonControl` whose absence or failure is logged and never fatal, and autostart written directly: a systemd user unit on Linux and a Task Scheduler logon task with `InteractiveToken` for the current user on Windows, installed and removed by the autostart setting
**Complexity**: 5 points
**Dependencies**: AZ-3883_windows_logon_task_spike, AZ-3889_ipc_schema_crate, AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3915
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c17

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `DaemonControl`, `DaemonStatus` (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — behaviour of the methods the menu calls (E-24/c19).

## Problem

Stock GNOME has no tray host, and a D-Bus failure can break the tray on any desktop; the daemon must sync fully without it (AC-26). The daemon must start at login without the settings window, and on Windows a service cannot own a tray, so it uses a logon task for the user (AC-26). The `auto-launch` crate is not used; autostart entries are written directly so their content is known and testable.

## Outcome

- Tray with tray-icon 0.26.1 (ksni backend on Linux): menu status, pause/resume, open settings (spawns the settings window, 15), open folder, pending holds, quit; the tooltip shows the sync state, `access_denied` and `watcher_degraded`.
- A missing tray host or a D-Bus failure is logged once and the daemon keeps running; the tray can be turned off by the `tray` setting.
- Linux autostart: a systemd user unit with `WantedBy=default.target` that starts the daemon binary; installed when autostart is on, removed when off.
- Windows autostart: a logon task for the current user with `InteractiveToken`, registered by the path found in E-24/c02 without a prompt; removed when autostart is off.
- An installer interface the control implementation calls on `set_settings(autostart)` (E-24/c19).

## Scope

### Included
- Tray menu, tooltip, non-fatal failure handling; both autostart installers with install, query and remove.

### Excluded
- macOS tray on the main thread and LaunchAgent [v1.1] (E-24/c22, E-24/c21); packages (deb, MSI) and signing (R19); the settings window (E-25).

## Acceptance Criteria

**AC-1: No tray host is not fatal (AC-26)**
Given LIN-VM stock GNOME without AppIndicator, and separately a broken session D-Bus
When the daemon starts its tray
Then the failure is logged once, the process stays alive, and IPC `status` still answers

**AC-2: Tray reflects status**
Given a tray host (KDE or Windows)
When a pair falls back to rescan-only and later a hold is entered
Then the tooltip shows the degraded pair and the menu's pending-holds entry lists the hold, and pause/resume from the menu change the pair's state through `DaemonControl`

**AC-3: Linux user unit**
Given autostart turned on, then off
When the installer runs
Then a user unit file with `WantedBy=default.target` that starts the daemon exists and is enabled, and after "off" the unit is disabled and removed

**AC-4: Windows logon task (AC-26)**
Given a standard user on WIN-VM and autostart on, then off
When the installer runs
Then a logon task for the current user with `InteractiveToken` is registered without a prompt, starts the daemon at the next logon in the user's session, and is removed when autostart is off (IT-14-20 Windows half)

**AC-5: Open settings**
Given the tray menu
When "open settings" is chosen
Then the settings binary is started as a separate process and the daemon keeps running if it fails to start

## Non-Functional Requirements

**Compatibility**
- Linux: GNOME (no tray), KDE; Windows 10 and 11 standard user. Lab results on LIN-VM and WIN-VM; absent hardware `NOT-RUN(H-*)`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Tray start with a failing backend | Logged once; no panic; daemon continues |
| AC-2 | Menu actions against a fake `DaemonControl`; tooltip text from a `DaemonStatus` | Calls made; tooltip names degraded pair and holds |
| AC-3 | Generated unit text | Contains `WantedBy=default.target` and the binary path |
| AC-4 | Generated task definition | Current user principal, `InteractiveToken`, logon trigger |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-3 | LIN-VM stock GNOME | IT-14-20 (Linux) | Tray failure logged; process alive; unit correct; removed when off | AC-26, FT-P-53 (H-LIN) |
| AC-4 | WIN-VM standard user | IT-14-20 (Windows) | Logon task registered without prompt; removed when off | AC-26, FT-P-53 (H-WIN) |

IT-14-20's "a new file reaches the server within 10 s with no tray" is a whole-daemon run and belongs to E-24/p01 (AC-1) and E-24/p02.

## Runtime Completeness

- Named capability: tray-icon 0.26.1 (ksni on Linux), systemd user units, Task Scheduler logon tasks (14 §5).
- Production code that must exist: real unit and task registration on the lab machines.
- Unacceptable substitutes: the `auto-launch` crate, an XDG autostart desktop file instead of the user unit, a Windows service.

## Constraints

- The tray is an adapter over `DaemonControl`; it holds no state of its own.
- Owner items: Windows signing (R19) affects SmartScreen, not this task's behaviour.

## Risks & Mitigation

**Risk 1: Logon task needs elevation**
- *Risk*: E-24/c02 finds no prompt-free path.
- *Mitigation*: AC-4 waits for the owner's answer recorded by E-24/c02.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AC-4 depends on the E-24/c02 result (standard-user registration unverified) | E-24 / owner | open | Medium |
| The binary path the unit and task point to depends on the package layout (deb, MSI), which no E-24 task defines | E-24 / CI | open | Low |
