# Linux keyring in a systemd user context and the fallback warning (spike)

**Task**: AZ-3885_linux_keyring_systemd_spike
**Name**: Linux keyring spike
**Description**: Find out how keyring 4.2.0 behaves when the daemon runs as a systemd user unit (desktop session, headless, locked keyring), how to tell "no Secret Service" from "locked" without hanging, and settle gap G-12 (the fallback warning text and the token file location)
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3885
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c03

## Problem

The token goes to the OS keyring (service `sunnycloud`, user = account id). On Linux without a Secret Service the daemon falls back to a 0600 file in a 0700 directory and warns once (R25, owner-accepted). Durable keyring behaviour when the daemon is started by a systemd user unit is unverified (14 §15 risk 2), and a keyring call that blocks without a session bus would stop sign-in (E-24/p01 risk 1). Gap G-12 also leaves the warning text and the file location open, and the two sources disagree: FT-P-61 says "user config dir", 14 §2 says `$XDG_STATE_HOME`.

## Outcome

- A written result, recorded in 14 §5 (TokenStore) and §15 risk 2: what keyring 4.2.0 does in each tested context, how long a failing call takes, and the detection rule the token store uses to choose the file fallback (and when it must not, for example a locked but present keyring).
- G-12 resolved: the exact WARN line text and the file location written into 14 §2 and §9 and into `tests/spec-gaps.md` (resolved with the date); FT-P-61 step 2 and IT-14-18 use the same location.
- Unblocks E-24/c15 and E-24/p01 AC-2.

## Scope

### Included
- Contexts on LIN-VM and the `desktop-linux` container: GNOME session with unlocked keyring; GNOME with locked keyring; systemd user unit started at login; headless user unit with lingering and no session bus; KDE (kwallet through Secret Service) if available.
- Store, load and delete of one entry; timing of each failure.
- The choice between the two locations named by the specs.

### Excluded
- The token store code (E-24/c15); Windows Credential Manager (works through keyring's Windows store; verified in E-24/c15).

## Acceptance Criteria

**AC-1: Behaviour per context recorded**
Given the five contexts above
When the spike stores, loads and deletes one entry in each
Then 14 §5 lists per context: success, error kind or block, and the time to fail

**AC-2: Detection rule without hanging**
Given the headless context with no session bus
When the token store decides between keyring and file
Then the recorded rule decides within a stated bound (the spike measures and names it) and never waits on a prompt; a locked keyring that is present is not treated as absent

**AC-3: G-12 resolved**
Given FT-P-61 ("user config dir") and 14 §2 (`$XDG_STATE_HOME`)
When the spike ends
Then one location and the exact WARN text are written in 14 §2 and §9, G-12 is marked resolved in `tests/spec-gaps.md`, and FT-P-61 and IT-14-18 name the same location

## Non-Functional Requirements

**Reliability**
- No context may leave the token in a file with mode other than 0600 or a directory other than 0700.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: decision outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Five Linux contexts | Store, load, delete; time to fail | Behaviour and rule recorded | FT-P-61, IT-14-18 |
| AC-3 | Spec texts | Location and WARN text | One answer in all three documents | FT-P-61 |

## Constraints

- The token never appears in logs, arguments or the environment during the spike (NFT-SEC-34).

## Risks & Mitigation

**Risk 1: Distribution differences**
- *Risk*: other desktops behave differently.
- *Mitigation*: the rule is based on the Secret Service answer, not on the desktop name.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| FT-P-61 and 14 §2 name different token file locations (G-12) | This spike | open | Low |
