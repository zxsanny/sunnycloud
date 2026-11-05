# Settings UI automation suite against the stub and the real daemon (AT-15-1..3, FT-P-82, PT-15-1..3)

**Task**: AZ-3911_settings_ui_suite
**Name**: Settings UI suite
**Description**: Run the settings window's acceptance, performance and accessibility tests through AT-SPI and UI Automation, against the stub daemon in CI and against the real daemon and server on the desktop lab, including the new FT-P-82 (an unconfirmed change reverts within 5 s)
**Complexity**: 3 points
**Dependencies**: AZ-3886_screen_reader_spike, AZ-3888_stub_daemon_ui_driver, AZ-3895_settings_app_shell, AZ-3896_account_screen, AZ-3897_folders_screen, AZ-3898_rules_screen, AZ-3899_settings_screen, AZ-3900_status_attention_screens, AZ-3909_native_dialogs, AZ-3903_delete_guard_holds, AZ-3908_token_store_device_login, AZ-3914_ipc_server_endpoint, AZ-3915_tray_autostart, AZ-3917_daemon_control_impl, AZ-3742_test_harness_skeleton
**Component**: 15 desktop_settings
**Tracker**: AZ-3911
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md`, `daemon_control.md` — the interface and behaviour both tiers exercise (E-24/c05, E-24/c19).

## Problem

Each screen task tests itself against the stub daemon. A stub can drift from the daemon, and several claims hold only against the real one: a setting reaching the daemon within 5 s (AC-16), a hold resolved with real files (AC-29), rules filtering real uploads (AC-66). The Step 1.7 review asked that this suite run against the real daemon, not only the stub. The cold-start target must also be confirmed on the finished binary on the reference machines. FT-P-82, a new system scenario from the approved mockup, needs the real daemon suspended while a change is pending.

## Outcome

- Two tiers: the stub tier on every merge (Linux containers with Xvfb or Wayland, and WIN-VM), the real-daemon tier on the desktop lab (the desktop profile of the E-31 harness) on H-LIN and H-WIN.
- AT-15-1, AT-15-2, AT-15-3, FT-P-82, PT-15-2, PT-15-3 and the IT-15-2 tree check pass against the real daemon; PT-15-1 passes on the finished binary on both reference machines.
- The AT set also passes inside a network namespace with only the loopback IPC.

## Scope

### Included
- The two tiers above and their CI wiring.
- AT-15-1 (add a pair with rules), AT-15-2 (mass-delete hold with DS-23), AT-15-3 (change a setting during a transfer), FT-P-82, PT-15-2, PT-15-3, IT-15-2 (six screens and accessible names), PT-15-1 on the finished binary.
- ST-15-2 step 2: the AT set inside a network namespace.

### Excluded
- The stub daemon and the UI driver (E-25/c04).
- The system scenarios owned by E-31 (FT-P-48..61, NFT-PERF-25, NFT-PERF-26).
- The manual NVDA and Narrator checks (E-25/c03).

## Acceptance Criteria

**AC-1: Add a folder pair with rules (AC-17, AC-66, AT-15-1)**
Given the real daemon signed in and an empty server account
When the user adds a pair through the OS folder dialog and the server picker, enters `*.tmp` and a bad line, fixes it and saves, then creates `x.tmp` and `y.txt` locally
Then the pair appears as active, the error marks the bad line until it is fixed, and only `y.txt` reaches the server

**AC-2: Mass-delete hold from the window (AC-29, AT-15-2)**
Given 300 synced files (DS-23) and the real daemon
When 200 files are deleted locally and the user presses "Restore from server", then the deletion is repeated and the user presses "Delete on server too" and confirms
Then the hold and counts show with the server unchanged, the 200 files return with equal hashes and the hold clears, and after the second decision the 200 files are deleted on the server

**AC-3: Change a setting during a transfer (AC-16, AT-15-3)**
Given the real daemon with a running transfer
When the user sets the bandwidth to 5000 and applies, then closes and reopens the window
Then the echoed value shows within 1 s, the transfer rate falls to the cap within 5 s, and the reopened window shows the same value

**AC-4: An unconfirmed change reverts (AC-16, FT-P-82)**
Given the real daemon signed in with one pair and the window open on H-LIN and H-WIN
When the harness suspends the daemon and the user toggles "Start at login", then the daemon is resumed and the user toggles it again, then sets the bandwidth to 5000
Then the first toggle returns to its previous value within 5 s with an inline error naming the change, the second shows the echoed value and "Applied" within 5 s with the autostart unit or logon task matching it, the bandwidth echo shows within 5 s, and the daemon PID is unchanged

**AC-5: Settings reach the daemon (AC-16, PT-15-3)**
Given the real daemon during a transfer
When 20 changes (bandwidth, hash throttle, tray, autostart) are made
Then the UI shows each as applied within 1 s (fail above 5 s), each effect is observable in the daemon within 1 s (fail above 5 s), and the daemon restarts 0 times

**AC-6: Memory of the window process (AC-15, PT-15-2)**
Given the window open on the Status tab for 5 min with status events at 1 per second
When RSS is sampled
Then RSS stays at or below 30 MB (fail above 60 MB) and grows by at most 2 MB over the 5 min

**AC-7: Six screens and accessible names (IT-15-2)**
Given a stub status with 2 pairs, 3 skipped, 2 escaped, one hold and an update available
When the AT-SPI and UI Automation trees are dumped
Then the six screens and their controls listed in IT-15-2 exist, and every Change, Remove, checkbox and number field has an accessible name and role

**AC-8: Cold start of the finished binary (AC-16, PT-15-1)**
Given REF-WIN-WEAK and REF-LIN-WAYLAND-NV with no workaround variable
When the shipped window is cold-launched 30 times per machine
Then p95 is at most 1 s (target 300 ms) and max at most 1.5 s (target 600 ms) on each; on other machines the result is `NOT-RUN(HW-REF)`

**AC-9: Offline run (ST-15-2 step 2)**
Given a network namespace with no interface except the loopback IPC
When the AT set runs
Then every test passes

## Non-Functional Requirements

**Performance**
- Targets as PT-15-1, PT-15-2, PT-15-3 above.

**Compatibility**
- Real-daemon tier on H-LIN and H-WIN; FT-P-82 in group X (exclusive).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: suite task | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Real daemon and server, empty account | AT-15-1 steps 1-4 | Pair active; line marked; only `y.txt` uploaded | AT-15-1, FT-P-60 (UI part) |
| AC-2 | DS-23, real daemon | AT-15-2 steps 1-3 | Hold; restore with equal hashes; delete after confirm | AT-15-2, FT-P-59 |
| AC-3 | Running transfer | AT-15-3 steps 1-3 | Echo <= 1 s; cap <= 5 s; same after reopen | AT-15-3, FT-P-49 |
| AC-4 | H-LIN, H-WIN; harness suspend | FT-P-82 steps 1-3 | 1 of 1 reverted; 2 of 2 shown <= 5 s; 0 restarts | FT-P-82 |
| AC-5 | Running transfer | 20 changes | <= 1 s UI, <= 1 s effect, 0 restarts | PT-15-3, NFT-PERF-26 |
| AC-6 | Status tab, 1 event/s | 5 min RSS | <= 30 MB; growth <= 2 MB | PT-15-2 |
| AC-7 | Stub status | Tree dumps | Screens and names present | IT-15-2 |
| AC-8 | Reference machines | 30 cold starts each | p95 <= 1 s; max <= 1.5 s | PT-15-1, NFT-PERF-25 |
| AC-9 | Network namespace | AT set | All pass | ST-15-2 |

## Constraints

- Cold-start and DPI claims count only on REF-WIN-WEAK and REF-LIN-WAYLAND-NV; VM results are informational (15 tests).
- The stub is allowed only at the process boundary; flows that need real sync use the real daemon and server with real PostgreSQL.

## Risks & Mitigation

**Risk 1: Reference hardware missing (R03)**
- *Risk*: AC-8 cannot run.
- *Mitigation*: `NOT-RUN(HW-REF)` is reported, never a VM result in its place.

## Runtime Completeness

- Named capability: the settings window against the real daemon.
- Production code that must exist: the real daemon binary and server stack in the real-daemon tier.
- Allowed external stubs: the stub daemon in the stub tier only.
- Unacceptable substitutes: running AC-1..AC-6 against the stub.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| FT-P-82 needs the harness agent's suspend action on desktop VMs (environment.md, "Daemon suspend"), owned by E-31 | E-31 | open | Low |
| G-AS-2: the bandwidth unit is open, so "5000" in AT-15-3, FT-P-82 and PT-15-3 has no unit | Owner | open | Medium |
