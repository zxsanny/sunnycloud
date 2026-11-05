# Local IPC server with endpoint ownership rules

**Task**: AZ-3914_ipc_server_endpoint
**Name**: IPC server
**Description**: Serve `DaemonControl` as newline-delimited JSON-RPC 2.0 on a local endpoint only the daemon's user can open (Linux 0600 socket in a 0700 directory under `$XDG_RUNTIME_DIR`; Windows pipe for the current user SID, remote clients refused, first-instance check), dispatch requests, push `StatusEvent` notifications to subscribers, and survive malformed input
**Complexity**: 5 points
**Dependencies**: AZ-3889_ipc_schema_crate, AZ-3652_initial_structure
**Component**: 14 desktop_daemon
**Tracker**: AZ-3914
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — methods, types, error mapping, framing and the Transport rules this task implements (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — method behaviour behind the dispatch (E-24/c19).

## Problem

The settings window changes settings and pairs through a local channel. If another OS user, or on Windows a remote client, can open that channel, they can redirect sync or read status (AC-48). A process that creates the pipe first can impersonate the daemon. The endpoint must be created so that only the current user can reach it, refuse to start when squatted, and never bring sync down because of a bad client.

## Outcome

- Linux: Unix socket mode 0600 inside a 0700 directory under `$XDG_RUNTIME_DIR`, owned by the daemon's user; other users are denied by the OS.
- Windows: named pipe with a security descriptor granting only the current user SID, remote clients rejected, created with `FILE_FLAG_FIRST_PIPE_INSTANCE`; a pre-created pipe makes the daemon refuse the endpoint, log an ERROR and report it, while sync continues.
- interprocess 2.4.7 local sockets; one JSON-RPC 2.0 object per line; requests dispatched to the `DaemonControl` implementation; `subscribe` connections receive one notification per `StatusEvent`.
- Malformed lines, unknown methods and undecodable params answered with the contract's JSON-RPC errors; the connection or the daemon never crashes or hangs because of them.

## Scope

### Included
- Endpoint creation and removal per OS (Linux, Windows), ownership and squatting checks, accept loop, per-connection framing, dispatch, notification fan-out, error answers.

### Excluded
- The methods' behaviour (E-24/c19); the client and its trust check (E-25); macOS endpoint [v1.1] (E-24/c22).

## Acceptance Criteria

**AC-1: Linux endpoint ownership (AC-48)**
Given the daemon running as user A in the `desktop-linux` container
When user A and user B try to connect
Then the socket has mode 0600 in a 0700 directory under A's `$XDG_RUNTIME_DIR`, A connects, and B is denied (IT-14-16)

**AC-2: Windows pipe ownership (AC-48)**
Given the daemon on WIN-VM as a standard user
When a second local user and a remote client try to connect
Then the pipe's security descriptor grants only the current user SID, and both are refused (IT-14-16, H-WIN)

**AC-3: Squatted endpoint**
Given a pipe (Windows) or socket path (Linux) pre-created by another process
When the daemon starts
Then it refuses to serve that endpoint, logs ERROR "endpoint refused" and reports it, and sync of the pairs continues

**AC-4: No token on the wire (ST-14-2 runtime half)**
Given a signed-in daemon
When every method is called and `subscribe` events are recorded for a session with pause, resume and a sync
Then the token string occurs 0 times in all recorded responses and events

**AC-5: Malformed input**
Given a client that sends an unparsable line, an unknown method, a `PairSpec` with two targets, and a 10 MiB line
When the server reads them
Then it answers -32700, -32601 and -32602 as the contract maps them, handles the large line without crashing or hanging (closing that connection is allowed), and other connections keep working

**AC-6: Events to subscribers**
Given two subscribed connections
When a pair is paused and resumed
Then each connection receives the two `StatusEvent` notifications, each with a full `DaemonStatus`

## Non-Functional Requirements

**Performance**
- Dispatch overhead small enough for `set_settings` p95 <= 200 ms end to end (PT-14-4, measured with E-24/c19).

**Security**
- The endpoint accepts no network listener of any kind; local only.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Framing reader with bad JSON, unknown method, two targets, oversize line | Contract error codes; no crash |
| AC-6 | Fan-out to two fake subscribers | Both receive both events |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Linux container, second OS user | IT-14-16 (Linux) | 0600 in 0700; other user denied | AC-48, NFT-SEC-32 |
| AC-2, AC-3 | WIN-VM second user, remote client, pre-created pipe | IT-14-16 (Windows) | Only current SID; remote refused; squat refused, sync continues | AC-48, NFT-SEC-32 (H-WIN) |
| AC-4 | Signed-in daemon, all methods and events recorded | ST-14-2 (runtime half) | 0 token occurrences | AC-48, NFT-SEC-34 |

## Runtime Completeness

- Named capability: interprocess 2.4.7 local sockets and Windows named pipes with a current-user security descriptor (14 §5 Local IPC).
- Production code that must exist: real endpoint creation with the stated modes, descriptor and first-instance flag.
- Unacceptable substitutes: a TCP listener on localhost; a pipe with the default security descriptor.

## Constraints

- IT tier: the server runs in-process with a real `DaemonControl` or the contract-conformant fake; the daemon-binary run is E-24/p01 (AC-1 of that task).
- Endpoint names follow `ipc_schema.md` open point 1 once fixed.

## Risks & Mitigation

**Risk 1: Stale socket file after a crash**
- *Risk*: the next start sees the old socket and treats it as squatted.
- *Mitigation*: the start-up rule for a leftover socket file in the daemon's own 0700 directory is decided in this task, written into `ipc_schema.md`, and covered by a restart-after-kill test (flagged below).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No maximum frame size is specified (`ipc_schema.md` open point 8); AC-5 only requires survival | E-24 | open | Low |
| The specs do not say how a leftover Linux socket from a crashed daemon is told apart from a squatted one | E-24 | open | Low |
| How "reports it" reaches the user when the endpoint itself is refused (tray? log only?) is not stated | E-24 | open | Low |

## Contract

This task implements the Transport section of the contract at `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` (produced by E-24/c05).
Consumers MUST read that file — not this task spec — to discover the endpoint rules and the wire format.
