# [v1.1] macOS tray on the main thread, notarised packaging hooks and IPC endpoint

**Task**: AZ-3920_macos_tray_packaging
**Name**: macOS tray and packaging
**Description**: Run the tray on the macOS main thread with the tokio runtime elsewhere, add the build hooks that produce a signed and notarised app bundle, and create the macOS IPC endpoint in the daemon's own 0700 directory
**Complexity**: 3 points
**Dependencies**: AZ-3915_tray_autostart, AZ-3914_ipc_server_endpoint, AZ-3919_macos_platform_fs_launchagent
**Component**: 14 desktop_daemon
**Tracker**: AZ-3920
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c22

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — Transport section, macOS endpoint row (E-24/c05).

## Problem

On macOS the tray must run on the main thread (14 §5), unlike Linux and Windows. `SMAppService` and Gatekeeper need a signed, notarised bundle. The IPC endpoint needs a macOS location with the same user-only guarantee as on Linux (AC-48).

## Outcome

- The daemon's main thread hosts the tray event loop on macOS; sync runs on the tokio runtime on other threads; a missing tray stays non-fatal.
- Build hooks that sign and notarise the app bundle when the owner's Apple credentials are present, and produce an unsigned development bundle otherwise.
- The macOS IPC endpoint: Unix socket in the daemon's own 0700 directory; another user is denied.

## Scope

### Included
- Threading change for macOS, packaging hooks, macOS endpoint.

### Excluded
- `PlatformFs` macOS, LaunchAgent, permission probe (E-24/c21); release workflow and credentials (owner, CI epic).

## Acceptance Criteria

**AC-1: Tray on the main thread**
Given the daemon on macOS
When it starts with a menu bar available
Then the tray icon and menu work, and IPC and sync keep working while the menu is open

**AC-2: Bundle hooks**
Given a build with signing credentials and one without
When the packaging hooks run
Then the first produces a signed bundle that passes notarisation and Gatekeeper on a clean machine, and the second produces a development bundle and says it is unsigned

**AC-3: macOS endpoint ownership (AC-48)**
Given the daemon running as user A
When user B tries to connect
Then the socket is in A's own 0700 directory and B is denied (NFT-SEC-32 macOS)

## Non-Functional Requirements

**Compatibility**
- Results on a macOS machine; absent hardware `NOT-RUN(HW-DEVICE)`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Runtime start order with the tray loop on the main thread (fake tray) | Runtime runs on other threads; no deadlock |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | macOS machine | Tray with menu open during sync | Sync and IPC continue | AC-26 (v1.1) |
| AC-2 | Credentials present and absent | Packaging hooks | Notarised bundle; dev bundle labelled | |
| AC-3 | Two macOS users | Endpoint ownership | Other user denied | AC-48, NFT-SEC-32 (v1.1) |

## Constraints

- v1.1 only; label `release:v1.1`.
- No Apple credential in the repository or CI except through the owner's release process.

## Risks & Mitigation

**Risk 1: Notarisation requirements change**
- *Risk*: a hook breaks on a new macOS.
- *Mitigation*: AC-2 runs on each release candidate.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Apple developer account and notarisation credentials are owner items | Owner | open | Medium |
| The macOS endpoint directory location is not named (`ipc_schema.md` open point 1) | E-24 | open | Low |
