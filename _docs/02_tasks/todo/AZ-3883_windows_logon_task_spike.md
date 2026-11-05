# Windows standard-user logon task without a prompt (spike)

**Task**: AZ-3883_windows_logon_task_spike
**Name**: Windows logon-task spike
**Description**: Find out whether a standard (non-admin) Windows user can register a Task Scheduler logon task with `InteractiveToken` for itself without an elevation prompt, and record the working registration path for the autostart installer
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3883
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c02

## Problem

AC-26 says the daemon starts on Windows through a logon task, because a Windows service cannot own a tray. The task must be created by the daemon for the current user, from XML or COM, with `InteractiveToken`. Whether a standard user can do this without a UAC prompt is unverified (14 §15 risk 3). The tray and autostart task (E-24/c17) cannot be built until the registration path is known.

## Outcome

- A written answer, recorded in 14 §15 risk 3 and §5 (Autostart) with the date and Windows builds tested: which registration path (Task Scheduler COM or `schtasks` with an XML definition) works for a standard user, with which principal and trigger settings, and whether removal works the same way.
- If no path works without elevation: the alternatives found and their trade-offs, put to the owner as one question (the spike does not pick a non-logon-task mechanism itself, because AC-26 names the logon task).
- Unblocks E-24/c17 and IT-14-20.

## Scope

### Included
- WIN-VM, Windows 10 and Windows 11 current builds, a fresh standard user and a user of a domain-less machine.
- Create, query and delete a logon task for the current user only, `InteractiveToken`, trigger at that user's logon.
- What the user sees (prompt or none) and the event log entries.

### Excluded
- The installer code (E-24/c17), MSI packaging and signing (R19).

## Acceptance Criteria

**AC-1: Registration path recorded**
Given a standard user on WIN-VM (Windows 10 and 11)
When the spike registers a logon task for that user with `InteractiveToken` by each candidate path
Then 14 §5 names the path that succeeds with no prompt, its principal and trigger settings, and the commands or COM calls in plain words; or it records that every path needs elevation

**AC-2: Removal and re-registration**
Given a registered task
When the same user removes it and registers it again
Then both succeed without a prompt, or the spike records the failure

**AC-3: Login start proven**
Given the registered task pointing at a small test program
When the user logs off and on
Then the program starts in the user's interactive session (it can show a tray icon), recorded with the event log entry

**AC-4: Fallback question if blocked**
Given AC-1 found no prompt-free path
When the spike ends
Then one owner question lists the alternatives with their costs, and IT-14-20's Windows half stays open until it is answered

## Non-Functional Requirements

**Compatibility**
- Results stated per Windows build; absent lab hardware is `NOT-RUN(H-WIN)`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: decision outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-3 | WIN-VM standard user | Register, log off and on, remove | No prompt; program starts at logon | IT-14-20, FT-P-53 |

## Constraints

- No `auto-launch` crate (14 §5).
- The test program is throwaway and never becomes part of the daemon.

## Risks & Mitigation

**Risk 1: Group policy differences**
- *Risk*: a managed machine blocks task creation.
- *Mitigation*: record the policy that blocks it; managed machines are not a v1.0 target unless the owner says so.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| If a standard user cannot register the task, AC-26's Windows half needs an owner decision | Owner | open | Medium |
