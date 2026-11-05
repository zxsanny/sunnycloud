# Gate S1: HTTP/2 windows and 1 Gbps through Caddy h2c (spike, R04)

**Task**: AZ-3799_s1_caddy_h2_spike
**Name**: Gate S1 server-side spike
**Description**: Measure single-stream and four-stream goodput through Caddy (client-facing HTTP/2, h2c to the API with 4 MiB/32 MiB windows) at 20 ms RTT, and record whether 112 MB/s is reachable and which R04 fallback applies
**Complexity**: 3 points
**Dependencies**: None
**Component**: 11 http_api
**Tracker**: AZ-3799
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c01

## Problem

AC-2 needs one large transfer at 90% of 1 Gbps. The API's own HTTP/2 windows (4 MiB per stream, 32 MiB per connection) cover the Caddy-to-API hop only when Caddy proxies over h2c. The client-facing hop is Caddy's, with Caddy's flow-control windows, and nobody has measured whether it sustains 112 MB/s at 20 ms RTT (R04, ADR-002, ADR-004). If it does not, the transfer design changes (a second connection in 13, or direct TLS on the API), so the answer is needed before the vertical slice is built.

## Outcome

- Goodput numbers for upload and download, one stream and four streams, at 20 ms RTT, for the path direct to the API-side probe and the path through Caddy with the release Caddyfile (`versions h2c`, request buffering off).
- A written conclusion in the R04 row of `risk_mitigations.md` and in the Risks table of the 11 component description: S1 passes through Caddy, or the chosen fallback (second connection from the client, direct TLS on the API) with its measured effect.
- A status line for gate S1 that E-31/c02 (gate map) and the client-side spike E-23/c02 can cite; numbers from a non-reference host are marked informational.

## Scope

### Included
- A throwaway probe server that applies the 11 HTTP/2 defaults (stream window 4 MiB, connection window 32 MiB, 100 streams) and streams memory-backed bytes, so disk and encryption are out of the measurement.
- Caddy as released (h2 to the client, h2c upstream, buffering off), and the same run with any client-facing window setting Caddy documents, if one exists.
- RTT shaping to 20 ms (netem or the E-31 shaper when it exists) and iperf3 calibration of the link.
- Recording results and the conclusion.

### Excluded
- The production accept loop and its settings test (E-21/c03, IT-11-18); PT-11-1 on the real stack (E-21/c20).
- The client-side decision for 4 lanes on one connection versus several (E-23/c02, NFT-PERF-04).
- Encryption and disk throughput (NFT-PERF-02, NFT-PERF-03 at system level).

## Acceptance Criteria

**AC-1: Both paths measured**
Given the probe with the 11 window defaults, the release Caddyfile and a link calibrated by iperf3 at 20 ms RTT
When 5 GiB is uploaded and downloaded on one stream and on four streams, directly to the probe and through Caddy
Then goodput per path, direction and stream count, the CPU of Caddy and of the probe, and the windows seen on the wire are recorded with the host description

**AC-2: Compared with the target**
Given the PT-11-1 target 117 MB/s and failure threshold 112 MB/s
When the results are compared
Then the record states pass or fail per path, and runs that are not on REF-SRV are labelled informational (`NOT-RUN(HW-REF)` for the gate itself)

**AC-3: Fallback recorded (AC-2)**
Given the results
When the conclusion is written
Then the R04 row and the 11 Risks table say either that S1 passes through Caddy, or which fallback is chosen (second connection, direct TLS on the API) with its measured goodput, and name E-23/c02 as the client-side follow-up

**AC-4: Probe stays out of production**
Given the probe
When the spike ends
Then no probe code is merged into the 11 crate; E-21/c03 takes over only the findings

## Non-Functional Requirements

**Performance**
- Target compared: PT-11-1 (117 MB/s target, 112 MB/s failure), NFT-PERF-02/03 single-stream rows.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (measurement task) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Probe, Caddy, 20 ms RTT, 5 GiB | Goodput per path, direction, stream count | Numbers and host recorded | PT-11-1, NFT-PERF-02, NFT-PERF-03 |
| AC-2 | Results | Comparison with 112 MB/s | Pass/fail per path; informational label off REF-SRV | AC-2 |
| AC-3 | Results | Document review | R04 row and 11 Risks state the outcome and fallback | gate S1 |
| AC-4 | Spike branch closed | Review of the 11 crate | No probe code merged | |

## Constraints

- Without a 1 GbE link at 20 ms RTT the spike stops and reports; it does not shorten the RTT or use loopback numbers as the gate.

## Risks & Mitigation

**Risk 1: Non-reference hardware**
- *Risk*: a CI runner or laptop does not predict REF-SRV.
- *Mitigation*: label the numbers informational; PT-11-1 repeats on REF-SRV (E-21/c20).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| REF-SRV and REF-LOAD are not provisioned (R03); gate S1 stays informational until they exist | Owner (hardware) | open | Medium |
| Whether Caddy exposes a setting for its client-facing HTTP/2 window is not stated in the docs | E-21 owner, deployment | open | Low |
