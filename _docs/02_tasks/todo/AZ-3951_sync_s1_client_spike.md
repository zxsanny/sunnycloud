# Sync core spike: gate S1 client side (HTTP/2 windows, Caddy hop, second connection)

**Task**: AZ-3951_sync_s1_client_spike
**Name**: Gate S1 client side
**Description**: Measure the client's large-transfer goodput through Caddy on the reference hardware, fix the HTTP/2 window and keep-alive defaults, and decide whether a second connection is needed
**Complexity**: 3 points
**Dependencies**: AZ-3799_s1_caddy_h2_spike
**Component**: 13_sync_core
**Tracker**: AZ-3951
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — transport rules and the `sync.h2_*` keys (open point 6).
- `_docs/02_document/tests/performance-tests.md` — NFT-PERF-04 (gate S1, client side).

## Problem

AC-2 needs one client to reach at least 90% of a 1 Gbps link (112 MB/s) with 4 lanes. All lanes share one HTTP/2 connection: one TCP flow, head-of-line blocking after loss (13 risk 1, R04/R03). The window sizes (8 MiB stream, 32 MiB connection) are start values sized above the 2.5 MB bandwidth-delay product. The server-side spike E-21/c01 fixes the Caddy hop and its fallback; nobody has measured the client side through that path, so the client defaults and the one-connection design are unproven.

## Outcome

- Measured client goodput up and down for 5 GiB (DS-09) through the chosen Caddy path, after `iperf3` calibration, at the start windows and at least two other window pairs, with and without `shape-wan` and on a lossy link.
- Recorded defaults for `sync.h2_stream_window`, `sync.h2_conn_window` and `sync.keepalive_interval`, and a decision on a second connection, written into the `sync_api` contract (open point 6) and 13 §3.
- E-23/c07 and E-23/c18b (PT-13-1) know the values to use; if a second connection is needed, a follow-up task is named.

## Scope

### Included
- Running the measurement on REF-SRV / REF-LOAD with the server settings chosen by E-21/c01.
- Comparing goodput, failed blocks and client CPU per window pair; one run with a lossy link profile.
- The decision record, including why a second connection is or is not needed.

### Excluded
- The server-side Caddy decision (E-21/c01) and server windows.
- Implementing the transport (E-23/c07) and the formal PT-13-1 run (E-23/c18b).

## Acceptance Criteria

**AC-1: Goodput measured per window pair**
Given REF-SRV, REF-LOAD, the Caddy path chosen by E-21/c01 and an `iperf3` calibration
When 5 GiB is transferred up and down with 4 lanes at the start windows and at two other window pairs
Then a table records goodput in MB/s, the share of calibrated goodput, failed blocks and client CPU for each run

**AC-2: Defaults and connection decision recorded (AC-2)**
Given the table
When the decision is recorded
Then the `sync_api` contract and 13 §3 state the window and keep-alive defaults and whether one connection meets at least 90% of calibrated goodput (>= 112 MB/s on 1 GbE); if it does not, the decision names the second-connection option and the follow-up task

**AC-3: Hardware gate honoured**
Given REF-SRV or REF-LOAD is not available
When the spike runs
Then the measurement is recorded as `NOT-RUN(HW-REF)` in `_docs/_process_leftovers/`, the start values stay in force, and no workstation run is reported as the result

## Non-Functional Requirements

**Performance**
- Target from AC-2 and NFT-PERF-04: >= 112 MB/s, 0% failed blocks, client below one full core.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (measurement and decision task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | REF-SRV, REF-LOAD, DS-09 | Goodput per window pair | Table recorded | PT-13-1, NFT-PERF-04 |
| AC-2 | Table | Decision record | Defaults and connection decision written | AC-2 |
| AC-3 | No reference hardware | Run attempt | `NOT-RUN(HW-REF)` recorded | |

## Constraints

- Timing results are valid only on the reference hardware (tier C-REF).
- Adding a second connection comes before any other transport change (13 risk 1).

## Risks & Mitigation

**Risk 1: Owner hardware not provisioned**
- *Risk*: REF-SRV is not named yet (performance gaps G1, G11), so the spike cannot measure.
- *Mitigation*: AC-3 keeps the start values and records the gap instead of a false result.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Reference server model and owner hardware not yet named (performance gaps G1, G11, R03) | Owner | open | Medium |
| A second connection would add a config key and change the in-flight memory budget (13 risk 3, R27); not decided here | E-23 / owner | open | Low |
