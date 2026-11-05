# Performance, security and acceptance tests PT-11-1..2, ST-11-1..4, AT-11-1..2

**Task**: AZ-3836_perf_security_acceptance
**Name**: HTTP API performance, security and acceptance tests
**Description**: Run the component's throughput and stalled-connection tests, the CSRF, injection, cross-account and worker-exposure security tests on the real stack, and the two acceptance tests (surface completeness, SIGTERM during 100 uploads)
**Complexity**: 3 points
**Dependencies**: AZ-3799_s1_caddy_h2_spike, AZ-3829_tree_transfer_feed_handlers, AZ-3834_csrf_idor_matrices, AZ-3835_integration_suite_crate_graph; notes: E-31/c09 (test epic E-31, decomposed at greenfield Step 9); E-31/c11 (test epic E-31, decomposed at greenfield Step 9); E-31/c13 (test epic E-31, decomposed at greenfield Step 9); E-31/c15 (test epic E-31, decomposed at greenfield Step 9); E-31/c19 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 11 http_api
**Tracker**: AZ-3836
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c20

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — operations, matrices input (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md`, `content_serving_headers.md` — leak and header rules (E-21/c09, c10).
- `_docs/02_document/contracts/11_http_api/http_server.md` — limits and shutdown order (E-21/c03).
- `_docs/02_document/contracts/11_http_api/server_info.md` — the 426 answer (E-21/c06).

## Problem

The component's acceptance (11 §15) needs measured numbers on reference hardware and attacks on the real stack: a single transfer at 90% of 1 Gbps through the stack (AC-2), 1,000 stalled connections costing at most 200 MB and slowing an upload at most 10% (AC-59), CSRF and cross-account matrices with real sessions (AC-58, AC-59), no header injection or leaks (AC-47), worker routes unreachable from the public path (AC-79), and no acked block lost when the API is stopped mid-upload (AC-38).

## Outcome

- PT-11-1 and PT-11-2 results recorded against their targets (`C-REF` only; elsewhere `NOT-RUN(HW-REF)`).
- ST-11-1 to ST-11-4 pass on the real stack.
- AT-11-1 and AT-11-2 pass.

## Scope

### Included
- PT-11-1: one and four h2c streams, 5 GiB GET and PUT, memory-backed port stub, `shape-perf` at 20 ms RTT.
- PT-11-2: DS-30 (1,000 stalled connections) during a DS-25 1 GiB PUT; `server-info` at 1,000 req/s for 2 minutes.
- ST-11-1: the E-21/c18 CSRF matrix with a real session cookie (mock-idp) and a read-only state probe.
- ST-11-2: CRLF in names and query strings, `Content-Disposition` forging, request smuggling (CL + TE), duplicate `Host`, oversized method names.
- ST-11-3: the E-21/c18 two-account matrix on the real stack, then a planted uncovered route in CI mode.
- ST-11-4: 30 path variants of `/api/v1/worker-jobs/**` (encoded slashes, `..`, case changes, trailing slash, `//api/v1/worker-jobs/status`) through the public path and with user tokens on `:8081`.
- AT-11-1 and AT-11-2.

### Excluded
- System scenarios NFT-PERF-02/03, NFT-SEC-09/13/15/50, NFT-RES-LIM-02 (E-31); the S1 measurement itself (E-21/c01).

## Acceptance Criteria

**AC-1: Single-stream throughput (AC-2)**
Given REF-SRV and REF-LOAD at 20 ms RTT and a memory-backed port stub
When one 5 GiB GET and one 5 GiB PUT run on one h2c stream, then on four
Then each run reaches at least 112 MB/s (target 117 MB/s), server CPU stays at or under 2 cores, and memory grows under 64 MiB; off REF-SRV the result is `NOT-RUN(HW-REF)` with the calibrated goodput recorded

**AC-2: Stalled connections and overhead (AC-59)**
Given the real stack and a 1 GiB PUT timed without load
When 1,000 connections handshake and send nothing while the PUT runs again, and separately `server-info` is called at 1,000 req/s for 2 minutes
Then RSS rises by at most 200 MB (target 100 MB), the PUT is at most 10% slower (target 5%), and `server-info` p95 is at most 10 ms (target 2 ms)

**AC-3: CSRF on the real stack (AC-58)**
Given a real session cookie from mock-idp
When the CSRF matrix runs against the real operations with foreign, sibling and missing origins
Then N of N state-changing operations answer 403 `csrf_rejected`, the read-only probe shows 0 state changes, and same-origin requests still work

**AC-4: No injection and no leaks (AC-47)**
Given the listed injection inputs on the real and stub stacks
When the raw responses are read
Then smuggling variants are rejected with 400 or a closed connection, no response header value contains a raw CR or LF, and no problem body contains SQL, a path or stack text

**AC-5: Cross-account matrix on the real stack (AC-59)**
Given two real accounts with one resource of every kind
When the two-account matrix runs, then a route without a matrix entry is planted and the generator runs in CI mode
Then 0 cross-account calls succeed (all 404), the matrix size equals the number of (operation, ID parameter) pairs, and the planted route fails the generator

**AC-6: Worker routes unreachable (AC-79)**
Given 30 path variants of the worker routes
When each is sent through the public path, and with user tokens to `:8081`
Then the public path answers 404 for every variant and `:8081` answers 403 for every variant without the worker bearer

**AC-7: Surface completeness (AC-58, AC-64)**
Given the real stack and the previous release's OpenAPI document
When the router is compared with `sunnycloud openapi`, the CSRF matrix runs, and an old client header is sent to a protected route
Then there are 0 undocumented routes, 403 on N of N state-changing operations, and 426 with `min_version`

**AC-8: SIGTERM during 100 uploads (AC-38)**
Given DS-34 and 100 concurrent uploads
When `ctl` sends SIGTERM to the API at about 50% of bytes, then the API restarts and clients retry
Then `/readyz` turns 503, long-polls are woken, the process exits within `shutdown_grace`, every file completes with hashes equal to the DS-34 manifest, and every block acked before SIGTERM is on disk with 0 zero-filled blocks

## Non-Functional Requirements

**Performance**
- PT-11-1: 117 MB/s target, 112 MB/s failure; CPU 1 core target, 2 cores failure.
- PT-11-2: RSS 100 MB target, 200 MB failure; upload delay 5% target, 10% failure; `server-info` p95 2 ms target, 10 ms failure.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (system-level runs) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | `C-REF`, stub port | PT-11-1 | >= 112 MB/s | AC-2, NFT-PERF-02, NFT-PERF-03 |
| AC-2 | DS-30, DS-25 | PT-11-2 | <= 200 MB, <= 10%, p95 <= 10 ms | AC-59, NFT-SEC-15, NFT-RES-LIM-02 |
| AC-3 | Mode R, mock-idp | ST-11-1 | 403 N of N; 0 changes | AC-58, NFT-SEC-09 |
| AC-4 | Mode S and R | ST-11-2 | 0 injected headers; 0 leaks | AC-47, NFT-SEC-38 |
| AC-5 | Mode R, two accounts | ST-11-3 | 0 cross-account success; planted route fails | AC-59, NFT-SEC-13 |
| AC-6 | Mode R, 30 variants | ST-11-4 | 404 public; 403 on `:8081` | AC-79, NFT-SEC-50 |
| AC-7 | Mode R, release fixture | AT-11-1 | 0 undocumented; N of N; 426 | AC-58, AC-64, FT-P-70 |
| AC-8 | Mode R, DS-34 | AT-11-2 | No acked block lost | AC-38, NFT-RES-03 |

## Constraints

- PT results on hardware other than REF-SRV and REF-LOAD are informational and labelled `NOT-RUN(HW-REF)`; they do not pass the gate.

## Risks & Mitigation

**Risk 1: Reference hardware missing (R03)**
- *Risk*: AC-1 and AC-2 cannot pass the gate.
- *Mitigation*: record informational numbers and the label; the owner provisions REF-SRV.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| REF-SRV and REF-LOAD are not provisioned (R03) | Owner (hardware) | open | Medium |
| If gate S1 (E-21/c01) chooses a fallback, PT-11-1 runs on the chosen path | E-21 owner | open | Low |
