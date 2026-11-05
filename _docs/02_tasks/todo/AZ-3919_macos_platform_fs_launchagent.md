# [v1.1] macOS PlatformFs, LaunchAgent and the access-denied probe

**Task**: AZ-3919_macos_platform_fs_launchagent
**Name**: macOS platform layer
**Description**: The macOS implementation of `PlatformFs` (`openat` with `O_NOFOLLOW`, dataless placeholders, Finder trash), autostart through a LaunchAgent registered with `SMAppService`, and a permission probe that reports a folder macOS denies within 10 s while other pairs keep syncing and resumes it within 10 s after the user grants access
**Complexity**: 5 points
**Dependencies**: AZ-3921_macos_placeholder_tcc_spike, AZ-3892_platform_fs_linux, AZ-3915_tray_autostart, AZ-3917_daemon_control_impl
**Component**: 14 desktop_daemon
**Tracker**: AZ-3919
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c21

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `DaemonStatus.access_denied` (E-24/c05).

## Problem

macOS arrives in v1.1. Its file system needs its own safe-write implementation, its iCloud placeholders are marked with a dataless flag, and its privacy controls (TCC) can deny the daemon access to a folder such as `~/Documents` without an error the user sees. AC-67: the tray and settings window show "access denied: allow access in System Settings" within 10 s, other folders keep syncing, and the folder resumes within 10 s after permission is granted.

## Outcome

- `PlatformFs` for macOS: per-component `openat` with `O_NOFOLLOW`, `is_placeholder` by the `SF_DATALESS` flag without opening data, `trash` to the Finder trash, file ids, case-insensitivity probe (APFS default is case-insensitive).
- Autostart through a LaunchAgent registered with `SMAppService` from the signed bundle, installed and removed by the autostart setting.
- A permission probe every `desktop.macos_permission_probe` (10 s) for each pair: a denial puts the pair in `DaemonStatus.access_denied`; the other pairs continue; a later successful probe resumes the pair.
- The `desktop.macos_permission_probe` row added to helper 01's key table.

## Scope

### Included
- macOS `PlatformFs`, LaunchAgent installer, permission probe and its status field.

### Excluded
- The tray on the main thread, notarised packaging and the macOS IPC endpoint (E-24/c22); the settings window message (E-25 v1.1 row).

## Acceptance Criteria

**AC-1: Safe writes on macOS (AC-37)**
Given the ST-14-1 swapper on APFS
When the daemon writes through `PlatformFs`
Then 0 files appear outside the root and redirected writes fail with `EscapesRoot`

**AC-2: Dataless placeholders**
Given iCloud-evicted files in a pair
When the pair scans and syncs
Then the files are `skipped_placeholder`, 0 bytes are hydrated and 0 deletes are propagated (NFT-RES-17 macOS)

**AC-3: Access denied reported and recovered (AC-67)**
Given two pairs, `~/Documents` (access not granted) and `~/Sync-ok`, with 10 files each
When the daemon starts, a file in `~/Sync-ok` is edited, and access to `~/Documents` is then granted
Then within 10 s `status.access_denied` lists the `~/Documents` pair, the edit in `~/Sync-ok` syncs within 5 s, and within 10 s after the grant the `~/Documents` pair resumes and its 10 files reach the server (IT-14-24, FT-P-75)

**AC-4: LaunchAgent autostart**
Given autostart on, then off, in a signed bundle
When the installer runs
Then the LaunchAgent is registered with `SMAppService` and starts the daemon at login, and is unregistered when off

## Non-Functional Requirements

**Compatibility**
- Current macOS on Apple silicon and Intel where available; absent hardware `NOT-RUN(HW-DEVICE)`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Probe state machine on `ManualClock` with a fake `PermissionDenied` | Denied listed within one probe period; resumed after a successful probe |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | macOS, APFS, swapper | ST-14-1 (macOS) | 0 outside the root | AC-37, NFT-SEC-31 (v1.1) |
| AC-2 | iCloud-evicted files | Placeholder skip | 0 hydration; 0 deletes | AC-32, NFT-RES-17 (v1.1) |
| AC-3 | Two pairs, one denied | IT-14-24 | Reported <= 10 s; other pair syncs; resume <= 10 s | AC-67, FT-P-75 (H-MAC) |
| AC-4 | Signed bundle | LaunchAgent | Registered; starts at login; removed when off | AC-26 (v1.1) |

## Runtime Completeness

- Named capability: `openat` with `O_NOFOLLOW`, `SF_DATALESS` detection, Finder trash, `SMAppService` LaunchAgent (14 §2, §5).
- Production code that must exist: real macOS calls on APFS.
- Unacceptable substitutes: path-string writes; a login item script instead of `SMAppService`.

## Constraints

- v1.1 only; label `release:v1.1`.
- The exact message text is shown by 15 and the tray; the daemon reports the state.

## Risks & Mitigation

**Risk 1: TCC behaviour differs by macOS version**
- *Risk*: the probe cannot tell denial from absence.
- *Mitigation*: E-24/c23 records the behaviour first.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-14-24 is still marked BLOCKED in `tests.md`; the `access_denied` field exists since the Step 5 gap pass, so the test text needs an update | E-24 | open | Low |
| Apple developer account and signing (bundle for `SMAppService`) are owner items | Owner | open | Medium |
