# Spike: OEM background behaviour, catch-up timing and the gate S9 protocol

**Task**: AZ-3988_oem_background_s9_spike
**Name**: OEM background and gate S9 spike
**Description**: Write the gate S9 measurement protocol for camera-backup latency and measure how Pixel, Samsung and Xiaomi defer content-URI triggers and the 15-minute catch-up, so the catch-up interval and the vendor help text rest on evidence
**Complexity**: 2 points
**Dependencies**: None
**Component**: 17 android_app (background work)
**Tracker**: AZ-3988
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c04

## Dependencies

### Document Dependencies
- `components/17_android_app/tests.md` IT-17-14, PT-17-1 (gate S9); `acceptance_criteria.md` AC-9; `components/17_android_app/description.md` §5 (Battery/OEM), §7, §10 (`android.catchup_interval`).

## Problem

AC-9 requires every new photo on the server within 60 s on a Pixel and publishes the Samsung and Xiaomi p95 (target 5 minutes) with the battery exemption on and Wi-Fi. OEM task killers defer or drop WorkManager triggers (R32), so the app relies on a 15-minute catch-up. There is no written protocol for the latency measurement (which timestamps, which device settings, how many photos), and no data on how long each vendor defers a trigger or a periodic job.

## Outcome

- The gate S9 protocol written into `components/17_android_app/tests.md` under IT-17-14 and PT-17-1: device preparation per vendor, the capture and arrival timestamps used, the clock source, the photo count and spacing, and the pass rule of AC-9.
- Per vendor, measured delays of a content-URI trigger and of the 15-minute periodic job, with the exemption on and off.
- Vendor help steps for E-27/c17, and a recorded decision on whether `android.catchup_interval` stays at 15 minutes.
- The full latency runs with the finished app stay lab runs (E-31/c45); this spike writes their protocol.

## Scope

### Included
- A throwaway probe app that logs trigger and periodic-job start times; no upload.
- Pixel, Samsung, Xiaomi (H-ANDROID); emulator API 34 as the baseline.
- Vendor settings that affect background work (autostart, sleeping apps, battery exemption).

### Excluded
- The real workers (E-27/c10) and the latency runs of the finished app (E-31/c45).

## Acceptance Criteria

**AC-1: Protocol written**
Given AC-9 and IT-17-14
When the spike ends
Then the protocol states device preparation per vendor, the capture and arrival timestamps and their clock source, 30 photos at 2-minute spacing per device, and the rule "Pixel maximum at most 60 s; Samsung and Xiaomi p95 published, at most 5 minutes as target"

**AC-2: Delays measured per vendor**
Given the probe on each phone
When at least 30 trigger events and 30 periodic-job events are logged with the exemption on and again with it off
Then the record shows the delay distribution (median, p95, maximum) per vendor and setting

**AC-3: Help steps and interval decision**
Given the measurements
When the spike ends
Then the record lists the vendor help steps for Xiaomi, Samsung and Huawei (Huawei marked unverified when no device was used), and states whether `android.catchup_interval` stays at 15 minutes and why

**AC-4: Missing hardware is visible**
Given a phone that is not available
When the spike ends
Then its legs are `NOT-RUN(HW-DEVICE)`, never PASS

## Non-Functional Requirements

**Performance**
- Measurement only; no target is checked here.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (protocol and measurements) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | tests.md IT-17-14 | Protocol review | All listed items present | AC-9, PT-17-1 |
| AC-2 | Probe on three phones | 30 + 30 events per setting | Distribution recorded | AC-9, R32 |
| AC-4 | Missing phone | Record review | `NOT-RUN(HW-DEVICE)` | |

## Constraints

- Phones are owner hardware (R03); the Pixel result decides AC-9.

## Risks & Mitigation

**Risk 1: Vendor behaviour changes with firmware**
- *Risk*: a firmware update changes deferral.
- *Mitigation*: the record names firmware versions.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 17 §5 names Huawei help pages, but no Huawei device is in the device matrix; its steps stay unverified | Owner | open | Low |
| Phones are owner hardware; the device legs may stay `NOT-RUN(HW-DEVICE)` | Owner | open | Low |
