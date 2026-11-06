# DaemonClient: endpoint trust check, framing, 5 s timeout, reconnect backoff, hostile answers

**Task**: AZ-3890_daemon_client
**Name**: Daemon client
**Description**: The settings window's only connection to the daemon: it refuses an endpoint not owned by the current user before sending a byte, speaks newline-delimited JSON-RPC 2.0, times every call out at 5 s, reconnects with a 250 ms to 2 s backoff, decodes typed errors and survives hostile answers
**Complexity**: 5 points
**Dependencies**: AZ-3889_ipc_schema_crate, AZ-3888_stub_daemon_ui_driver, AZ-3763_sc_config
**Component**: 15 desktop_settings
**Tracker**: AZ-3890
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — transport, trust check, methods, errors, JSON-RPC mapping (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — every method answers within 5 s (E-24/c19).

## Problem

Another local user could create the pipe or socket name before the daemon and capture what the window sends (NFT-SEC-32 step 3, AC-48). The window must check ownership before it sends anything. The daemon may also be absent, slow or, on a trusted endpoint, send malformed answers; the window must neither crash nor hang. Every screen calls the daemon through this one client.

## Outcome

- An endpoint that is not owned by the current user is refused with `EndpointUntrusted`, and it receives 0 bytes.
- Every call ends within 5 s with a result, a typed error or a timeout.
- With no endpoint the client reports `DaemonNotRunning` and reconnects on its own when the daemon appears.
- Malformed or oversize frames are rejected and logged; the window stays alive.

## Scope

### Included
- Endpoint location by the path rule (see Flagged concerns) and the trust check of `ipc_schema.md` (Unix: socket owner and mode; Windows: pipe server SID).
- Newline-delimited JSON-RPC 2.0: request ids, matching answers to callers, notifications after `subscribe`.
- The 5 s request timeout; a late answer after a timeout is dropped.
- Reconnect with a backoff from 250 ms growing to at most 2 s.
- Decoding every `ControlError` variant with its fields, `Internal{correlation_id}`, and an unknown kind as a generic error.
- Client-side errors `DaemonNotRunning` and `EndpointUntrusted`.
- Logging: WARN for an untrusted endpoint and a lost connection, ERROR for protocol violations; no user path or user code at INFO.

### Excluded
- Showing errors and the daemon-not-running view (E-25/c06, E-25/c11).
- The status refresh policy (one `status` at open, `subscribe`, 2 s fallback) (E-25/c11).
- The thread that hosts the client and the hand-over of results to the UI thread (E-25/c06).

## Acceptance Criteria

**AC-1: Untrusted endpoint refused before any byte (AC-48, IT-15-12)**
Given a squatter at the endpoint path owned by another OS user (Linux: wrong owner and mode 0666; Windows: a pipe whose server SID differs), and a squatter with the right owner in a group-writable directory
When the client connects
Then it returns `EndpointUntrusted`, the squatter receives 0 bytes, a WARN log line says "endpoint untrusted", and the client offers no call that skips the check

**AC-2: Framing and matching**
Given a trusted stub that answers three concurrent requests out of order and sends notifications between them
When the client makes the three calls after `subscribe`
Then each caller gets its own answer by id and the notifications arrive in the order sent

**AC-3: 5 s timeout**
Given a stub that never answers one call and answers another after 6 s
When the client makes both calls
Then both callers get a timeout at 5 s, and the late answer is dropped without reaching a caller

**AC-4: Daemon not running and reconnect (IT-15-10)**
Given no endpoint at the path
When the client starts, and the stub endpoint appears after 3 s
Then the client reports `DaemonNotRunning`, its attempts are spaced from 250 ms growing to at most 2 s, and it connects and answers `status` once the endpoint exists, without a restart of the window

**AC-5: Hostile answers on a trusted endpoint (ST-15-1)**
Given a trusted stub that answers with a 10 MB frame, invalid JSON, a wrong id and error text `<b>x</b>{@html}<script>`
When the client receives each
Then it does not crash or hang longer than 5 s, rejects the malformed frames with an ERROR "protocol violation" log line, frees the memory of the rejected frame, and passes the error text on unchanged as plain data

**AC-6: Typed errors decoded**
Given each `ControlError` variant of `ipc_schema.md`, `Internal{correlation_id: "c-77"}` and an unknown kind
When the client decodes them
Then each variant arrives typed with its fields (`RulesInvalid{line}`, `SettingInvalid{key}`, `UpgradeRequired{message}`), the internal error keeps "c-77", and the unknown kind becomes a generic error

## Non-Functional Requirements

**Reliability**
- No call waits longer than 5 s (compile-time constant, 15 §10); backoff 250 ms to 2 s (15 §10).

**Security**
- The trust check runs on every connect and reconnect.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Frame codec: split lines, partial reads, ids | Frames and ids decoded correctly |
| AC-4 | Backoff schedule | Starts at 250 ms, never above 2 s |
| AC-6 | Error decoding per variant | Typed errors with fields |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Second OS user; squatters on Linux and Windows | Connect | `EndpointUntrusted`; 0 bytes to squatter; WARN | IT-15-12, NFT-SEC-32 steps 3-4 |
| AC-2 | Stub with out-of-order answers | Three calls after `subscribe` | Answers matched; events in order | |
| AC-3 | Stub silent and 6 s late | Two calls | Timeouts at 5 s; late answer dropped | AC-16 |
| AC-4 | No endpoint; stub after 3 s | Start, wait | `DaemonNotRunning`; backoff; connects | IT-15-10 |
| AC-5 | Hostile stub scripts | Receive each | No crash; ERROR log; text unchanged | ST-15-1 |
| AC-6 | Stub error scripts | Calls | Typed errors decoded | IT-15-1 |

## Constraints

- The client depends on `ipc_schema` only, never on the daemon crate, `sync-core` or an HTTP client (module-layout).

## Risks & Mitigation

**Risk 1: A trust check that differs from the daemon's endpoint rules**
- *Risk*: the window refuses the real daemon.
- *Mitigation*: E-25/c14 runs the flows against the real daemon on Linux and Windows.

## Runtime Completeness

- Named capability: the local endpoint trust check.
- Production code that must exist: the owner and mode check (Unix) and the pipe server SID check (Windows) on the real OS objects.
- Allowed external stubs: the stub daemon as the counterpart.
- Unacceptable substitutes: a check of the path name only, or a check skipped in a test build.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The endpoint path rule: 15 §1 says helper 01 provides it, but helper 01 and `sc_config` have none; `ipc_schema.md` open point 1 proposes it lives in `ipc_schema`. This task uses whichever E-24/c05 freezes | E-24 / E-01 owners | open | Medium |
| No maximum frame size is set (`ipc_schema.md` open point 8); the client needs one to reject the 10 MB frame of ST-15-1 early | E-24 / E-25 owners | open | Medium |
