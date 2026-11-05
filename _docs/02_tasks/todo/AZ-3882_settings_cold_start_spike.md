# Gate S3: settings window cold start on the reference machines with the software renderer (spike)

**Task**: AZ-3882_settings_cold_start_spike
**Name**: Gate S3 cold-start spike
**Description**: Measure the cold start of a Slint software-renderer window built with the release profile on REF-WIN-WEAK and REF-LIN-WAYLAND-NV, check fractional scaling, and record whether gate S3 passes or the Tauri fallback trigger fires
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 15 desktop_settings
**Tracker**: AZ-3882
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c01

## Problem

AC-16 requires the settings window to open within 1 s at p95, cold, on a 2019 Windows laptop with an HDD-class disk and on Ubuntu LTS Wayland with NVIDIA, with no environment-variable workaround. The MVE measured the software build at 47-61 ms and about 17 MB, but Windows cold start and fractional DPI (125/150/175 %, Windows per-monitor DPI) are unmeasured (15 §7, §15 risk 1). ADR-012 names "cold start over 1 s at p95 after trimming" as fallback trigger 1 for Tauri 2 + Svelte. The decision has to be made before the screens are built on Slint.

## Outcome

- Cold-start p95 and max per reference machine, with run count, build id and OS version, recorded in `components/15_desktop_settings/description.md` §15 risk 1 as the gate S3 result.
- A verdict: gate S3 passed, or fallback trigger 1 recorded for the owner.
- A fractional-scaling note per scale factor, recorded in 15 §7.

## Scope

### Included
- A prototype window with the window frame and one screen of the approved mockup (`ui_mockups/desktop-settings.html`), built with the planned release profile and the software renderer only.
- The NFT-PERF-25 method: launch to the first frame that accepts input, probed by UI Automation (Windows) and AT-SPI (Linux); cold = file cache dropped or reboot before each run; 40 runs per machine (covers PT-15-1's 30 and FT-P-48's 20).
- Window RSS at 5 s after open (informational).
- Rendering at 125, 150 and 175 % scaling, and a Windows per-monitor DPI move.

### Excluded
- The full app (E-25/c06 onwards) and the cold-start check of the finished binary (E-25/c14, PT-15-1).
- Building the Tauri fallback.

## Acceptance Criteria

**AC-1: Measured on both machines (AC-16)**
Given REF-WIN-WEAK and REF-LIN-WAYLAND-NV with the daemon running and no workaround variable set
When the prototype is cold-launched 40 times per machine
Then the record holds p95 and max per machine, the run count, the build id and the OS version, and the run log shows that no variable such as `SLINT_BACKEND` or `WEBKIT_DISABLE_*` was set

**AC-2: Verdict recorded**
Given the measurements
When p95 is at most 1 s on both machines
Then gate S3 is recorded as passed with the targets of PT-15-1 (p95 300 ms, max 600 ms) marked met or missed; when p95 is above 1 s on either machine, fallback trigger 1 is recorded for the owner instead of a rerun (NFT-PERF-25)

**AC-3: Fractional scaling**
Given the prototype at 125, 150 and 175 % scaling and a window moved between monitors of different DPI on Windows
When each case is rendered
Then a screenshot per case and a note on clipped or misplaced controls are stored with the record in 15 §7

**AC-4: Missing hardware**
Given a reference machine that is not provisioned (R03)
When the spike runs without it
Then that machine's result is `NOT-RUN(HW-REF)` and any VM number is recorded as informational only

## Non-Functional Requirements

**Performance**
- Targets as PT-15-1: p95 <= 300 ms (fail > 1 s), max <= 600 ms (fail > 1.5 s).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: measurement outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | REF-WIN-WEAK, REF-LIN-WAYLAND-NV, cold | 40 cold launches per machine | p95 and max recorded; no workaround variable | PT-15-1, NFT-PERF-25, FT-P-48 |
| AC-2 | Measurements | Verdict | Passed, or trigger 1 recorded | ADR-012 |
| AC-3 | 125/150/175 %, per-monitor DPI | Render and capture | Screenshots and notes stored | 15 §7 |
| AC-4 | Machine absent | Run without it | `NOT-RUN(HW-REF)` | R03 |

## Constraints

- Software renderer only; no femtovg or Skia in the prototype (ADR-012).
- Claims are valid only on the two reference machines (environment.md row 16).

## Risks & Mitigation

**Risk 1: Reference laptops not provisioned (R03)**
- *Risk*: the gate cannot be decided.
- *Mitigation*: AC-4 records `NOT-RUN(HW-REF)`; the owner provides the hardware before gate S3.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Run counts differ between PT-15-1 (30), FT-P-48 (20) and NFT-PERF-25 (40); the spike uses 40, which satisfies all three | Test-spec owner | open | Low |
| Reference hardware REF-WIN-WEAK and REF-LIN-WAYLAND-NV is owner-provided (R03) | Owner | open | Medium |
